---
title: 'Story 13.1: Module trees with unique path ids'
type: 'feature'
created: '2026-10-24'
status: 'done'
baseline_revision: '82229f55fc10f5cea0d727a2e9724ad1956f9d2b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred:
  - summary: >-
      epic-last.py keeps an epic open while a maintainer-gated or never-done story remains, and silently ignores failed git fetch/show, so cargo hack can be skipped for a whole epic.
    evidence: |-
      tools/story-loop/epic-last.py counts every non-done story of the epic (locally and on origin/main) and does not check the return codes of git fetch or git show; [Maintainer-gated] stories never reach done in unattended runs. Pre-existing tooling from commit 2defad7, not Story 13.1.
    location: >-
      tools/story-loop/epic-last.py:19-32
    severity: medium
  - summary: >-
      gates.sh treats an epic-last.py crash as "not last" and still prints ALL-OK, and epic-last.py picks any in-progress/review row instead of the story being gated.
    evidence: |-
      gates.sh only tests epic-last.py's exit status for zero; a Python traceback (exit 1) skips cargo hack silently. With several lanes, an in-progress story of another epic can decide the hack step. Pre-existing tooling from commit 2defad7.
    location: >-
      tools/story-loop/gates.sh:27
    severity: medium
  - summary: >-
      epic-last.py has no fixture test for its last-story branch, so a regression can stop cargo hack for whole epics with no gate failing.
    evidence: |-
      Verification-gap layer: a repo search for epic-last finds only gates.sh, README.md and AGENTS.md; no test runs the script. Pre-existing tooling from commit 2defad7.
    location: >-
      tools/story-loop/epic-last.py
    severity: medium
  - summary: >-
      The debug-builds-allowed policy (AGENTS.md "Running and verifying", dropped FAIL:debug-dir gate) contradicts maintainer-decisions.md section 6 "Release builds only", and AGENTS.md still says to run cargo hack before calling work done while also saying agents never run it.
    evidence: |-
      Commit 2defad7 rewrote AGENTS.md and removed `test ! -d target/debug` from gates.sh without updating maintainer-decisions.md section 6; the template troubleshooting text still says release only. The fix edits agent-context files and the decisions page, so it is deferred to the maintainer.
    location: >-
      AGENTS.md; tools/story-loop/gates.sh; _bmad-output/planning-artifacts/maintainer-decisions.md
    severity: medium
  - summary: >-
      bg.sh has job-lifecycle races: two attached callers race on cleanup (second gets exit ""), a killed job leaves callers waiting forever, a command that changes the tree gets a new key on rerun, and tail -f is killed after a fixed sleep.
    evidence: |-
      tools/story-loop/bg.sh moves and removes the job directory after the first waiter reads status, has no liveness check on the job pid, and keys jobs on the tree state. Pre-existing tooling from commit 2defad7.
    location: >-
      tools/story-loop/bg.sh:8-30
    severity: low
---

<intent-contract>

## Intent

**Problem:** Module ids are free strings today (`Module` rustdoc: "Ids are not validated yet"): two Modules may share an id, a nested Module's id need not reflect where it sits in the tree, and the Usage Skill marks the check `[planned]`. FR-44 requires unique, path-like ids that every registration carries in logs and errors; Story 13.3 (runtime enable/disable) keys its state by these ids.

**Approach:** Keep the author-written id as the Module's full path (`Module::new("profile").module(Module::new("profile/badges"))`) and validate the mounted trees in `Registry::fold` (the single fold `App::build`, `--publish --dry-run` and the testkit share): grammar, tree position (a child is its parent's id plus one segment), and per-App uniqueness, each as a typed `BuildViolation`. Prove with a 3-level tree that registrations fold, Middleware runs parent before child at every level, and spans and Error Sink reports carry the full-path Module id.

## Boundaries & Constraints

**Always:**
- Module id grammar: one or more segments joined by single `/`; a segment is 1+ characters of `a-z`, `0-9`, `_`, `-`; whole id 1..=64 characters. Errors index by character (like `AppIdError`).
- Tree position: a root Module (mounted with `AppBuilder::module`, plugins included) has exactly one segment; a child id must equal `<parent id>/<one segment>`.
- Uniqueness: one id per App across all mounted trees; each later occurrence in fold pre-order is one `BuildViolation::DuplicateModule`. Duplicates are checked for every id, valid or not.
- All Module-id violations are reported (never short-circuit), first in `Registry::fold`'s violation list, in fold pre-order; for one Module its `InvalidModuleId` precedes its `DuplicateModule`. All other fold checks still run.
- Ids stay `&'static str`; `Module::new` stays infallible and non-panicking (checks happen at App build, maintainer decisions §4: build time is the earliest point for a run-time constructor and the only point for tree position and uniqueness).
- No metric gains a `module` label (AD-29's catalog lists none); spans and `ErrorReport.module` carry the id.
- Same Module value mounted on two different Apps stays legal (per-App uniqueness).
- Every public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt`, and updates the Skill's `API hash` line in the same commit.

**Never:**
- No relative/auto-prefixed child ids, no id rewriting at `.module(..)`, no owned/`Arc<str>` ids.
- No new rule id in `docs/discord-rules.md` (framework convention, not a Discord rule; like `DuplicateTemplate`).
- No Boot Flags, enable/disable, `ModuleStateStore`, rate limits or hooks (Stories 13.2-13.5).
- No change to fold order, Middleware chain semantics or FR-57's "lexicographically first Module id" rule.

## I/O & Edge-Case Matrix

Grammar (`check_id`, run on every mounted Module):

| # | Id | Expected |
|---|----|----------|
| G1 | `profile` | Ok |
| G2 | `a/b/c` | Ok (grammar) |
| G3 | `_`, `-`, `0`, `a_b-9` | Ok |
| G4 | `""` | `Empty` |
| G5 | 64 × `a` | Ok |
| G6 | 65 × `a` | `TooLong { len: 65 }` |
| G7 | 65 × `ü` | `TooLong { len: 65 }` (chars, checked before chars are scanned) |
| G8 | `Profile` | `InvalidChar { index: 0, ch: 'P' }` |
| G9 | `a b` | `InvalidChar { index: 1, ch: ' ' }` |
| G10 | `shop.admin` | `InvalidChar { index: 4, ch: '.' }` |
| G11 | `a:b` | `InvalidChar { index: 1, ch: ':' }` |
| G12 | `ü/🙂` | `InvalidChar { index: 0, ch: 'ü' }` |
| G13 | `a/🙂` | `InvalidChar { index: 2, ch: '🙂' }` |
| G14 | `/` | `EmptySegment { index: 0 }` |
| G15 | `/a` | `EmptySegment { index: 0 }` |
| G16 | `a/` | `EmptySegment { index: 2 }` |
| G17 | `a//b` | `EmptySegment { index: 2 }` |
| G18 | `a//B` | `EmptySegment { index: 2 }` (first problem left to right wins) |
| G19 | `A//b` | `InvalidChar { index: 0, ch: 'A' }` |

Tree position (only when grammar passed) and uniqueness:

| # | Mounted tree(s) | Expected violations (in order) |
|---|-----------------|--------------------------------|
| T1 | root `ops` → `ops/audit` → `ops/audit/log` | none |
| T2 | root `a/b` | `InvalidModuleId { id: "a/b", reason: PathAtRoot { segments: 2 } }` |
| T3 | root `r1` → child `c1` | `InvalidModuleId { id: "c1", reason: NotUnderParent { parent: "r1" } }` |
| T4 | root `a` → child `a/b/c` | `NotUnderParent { parent: "a" }` (more than one segment added) |
| T5 | root `a` → child `ab` | `NotUnderParent { parent: "a" }` (prefix without `/`) |
| T6 | root `a` → child `a` | `NotUnderParent { parent: "a" }`, then `DuplicateModule { id: "a" }` |
| T7 | root `A` → child `A/b` | `InvalidModuleId { "A", InvalidChar{0,'A'} }`, `InvalidModuleId { "A/b", InvalidChar{0,'A'} }` |
| T8 | root `A` → child `a/b` | `InvalidChar` for `A`, `NotUnderParent { parent: "A" }` for `a/b` |
| T9 | roots `shop`, `shop` | `DuplicateModule { id: "shop" }` once |
| T10 | `m.clone()` and `m` mounted (`m` = `p` → `p/c`) | `DuplicateModule` for `p`, then for `p/c` |
| T11 | root `p` → children `p/x`, `p/x` | `DuplicateModule { id: "p/x" }` once |
| T12 | roots `""`, `""` | `InvalidModuleId { "", Empty }` twice, `DuplicateModule { "" }` once, interleaved per Module |
| T13 | same tree T1 on two separate Apps | both build |
| T14 | plugin `install` mounts root `ops` while the builder already mounted `ops` | `DuplicateModule { id: "ops" }` |
| T15 | invalid ids plus a GW-05 handler | id violations first, then `MissingIntent` (all reported) |

Display strings (exact, tested):
- `InvalidModuleId`: ``invalid Module id `{id}`: {reason}``
- `Empty`: ``the id is empty: use segments of a-z, 0-9, `_` and `-` joined by `/`, such as `profile/badges` ``
- `TooLong`: `the id has {len} characters: shorten it to at most 64`
- `InvalidChar`: ```{ch}` at index {index} is not allowed: use only a-z, 0-9, `_` and `-`, with `/` between segments``
- `EmptySegment`: ``empty segment at index {index}: separate segments with exactly one `/`, with none at the start or end``
- `PathAtRoot`: ``it is mounted at the root with {segments} segments: a root id is one segment, so nest it with `.module(..)` in its parent Module``
- `NotUnderParent`: ``it is nested in Module `{parent}`, so it must be `{parent}/<name>` with exactly one segment after `{parent}/` ``
- `DuplicateModule`: ``Module id `{id}` is mounted twice: give each Module its own id (a Module tree is mounted at most once per App)``

</intent-contract>

## Code Map

- `crates/noctis-core/src/module.rs` -- `Module { id: &'static str, .., children }`, `Module::new` (rustdoc line "Ids are not validated yet" ~L107), `module(child)` (~L289). Home for `ModuleIdError` and `pub(crate) fn check_id(id: &str) -> Result<(), ModuleIdError>` (+ `const MAX_ID_LEN: usize = 64`).
- `crates/noctis-core/src/registry.rs` -- `Registry::fold(roots, requested, strings)` (~L150): pre-order stack of `(module, ancestors, parent_chain)`; parent id = `ancestors.last()` (`None` = root). Add id checks here, first in `violations`; update the fold rustdoc listing violation order. `Entry/RouteEntry.module` already = `module.id()`.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (`#[non_exhaustive]`, ~L216): add `InvalidModuleId { id: &'static str, reason: ModuleIdError }` and `DuplicateModule { id: &'static str }`, each with a rustdoc doctest asserting `to_string()`. Pattern: `InvalidAppId { reason: AppIdError }` (~L289), `DuplicateTemplate` (~L539).
- `crates/noctis-core/src/namespace.rs` ~L156 -- `AppIdError` (thiserror, `#[non_exhaustive]`, char index): template for `ModuleIdError`.
- `crates/noctis-core/src/lib.rs` ~L231/268 -- root re-exports; add `ModuleIdError`. `crates/noctis/src/lib.rs` ~L592 -- facade list; add `ModuleIdError`.
- `crates/noctis-core/src/app.rs` ~L2516 -- `build()` calls `Registry::fold` (no change needed); `AppBuilder::module` rustdoc ~L2066: mention id rules. `crates/noctis-core/src/lib.rs` ~L515 `route_table` also folds (gets the checks for free).
- Already in place (verify, do not rebuild): `module` span field on `noctis.interaction` (`pipeline.rs` ~L863/880, ~L1047/1176/1188, autocomplete ~L1567/1580; `pipeline/component.rs` ~L578/693), `noctis.event` (`worker.rs` ~L439); `ErrorReport.module` (`sink.rs` ~L148) set by pipeline, worker, render (`render.rs` ~L202), Middleware reports. `middleware::Chain::extend` builds parent-before-child chains in fold.
- `crates/noctis-core/src/observe/tests.rs` ~L60 `catalog_static` -- add the "no `module` label" assertion.
- Tests whose nested ids break the new rules (migrate to full paths; adjust expected strings): `crates/noctis-core/src/registry/tests.rs` (`fold_order_and_ancestors` p→c→g; Matrix 8 `ids_unvalidated` → delete, replaced by new tests; ~L970-1060 `root`→`c`→`g`, `shared` nested under `p` and `b` → two Modules `p/shared`, `b/shared`), `crates/noctis-core/src/app/tests.rs` ~L655-690 (`r1`→`c1`→`g1`), `crates/noctis-core/src/pipeline/tests/{middleware.rs,guards.rs}` (`root`→`child`/`g`/`stop`), `crates/noctis-testkit/tests/middleware.rs` ~L597/645/757 (`root`→`child`). Run the full test suite to find any others.
- `crates/noctis-testkit/tests/middleware.rs` -- FakeGateway/FakeDiscordHttp harness (`start`, `r1`/`c1` Middleware logging) to copy for the new end-to-end test; `crates/noctis-testkit/tests/observability.rs` L40-80 -- log `Capture` writer pattern for span fields.
- `examples/project-template/src/modules/profile/mod.rs` L23 -- `module()`; `handlers::Badge` (`/badge` + autocomplete) moves to child `Module::new("profile/badges")`.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md` §1 -- replace the "Ids are not validated yet … [planned]" bullet; `references/troubleshooting.md` ~L115 build-error table; `SKILL.md` L23 `API hash`; `examples/project-template/AGENTS.md` L24/L59 area.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/module.rs` -- add `pub enum ModuleIdError` (`Debug, Clone, Copy, PartialEq, Eq, thiserror::Error`, `#[non_exhaustive]`) with variants `Empty`, `TooLong { len: usize }`, `InvalidChar { index: usize, ch: char }`, `EmptySegment { index: usize }`, `PathAtRoot { segments: usize }`, `NotUnderParent { parent: &'static str }`, each field documented, Display strings exactly as in the matrix; add `pub(crate) fn check_id` (grammar only: Empty, then TooLong by char count, then one left-to-right scan); replace the "Ids are not validated yet" sentence in `Module` rustdoc and `Module::new`/`Module::module` docs with the rules (link `BuildViolation::InvalidModuleId`/`DuplicateModule`) -- the typed reason.
- `crates/noctis-core/src/error.rs` -- add the two `BuildViolation` variants with doctests -- typed build errors naming the id.
- `crates/noctis-core/src/registry.rs` -- in `fold`, before any other violation: for each popped Module run `check_id`, then (grammar ok) the position check against `ancestors.last()`, then the `HashSet<&'static str>` duplicate check; push in that order; update fold rustdoc -- one fold for build, dry run and testkit.
- `crates/noctis-core/src/lib.rs`, `crates/noctis/src/lib.rs` -- re-export `ModuleIdError` -- public typed reason.
- `crates/noctis-core/src/module.rs` or `crates/noctis-core/src/registry/tests.rs` -- unit tests G1-G19 (table-driven) and T1-T12, T15 via `Registry::fold`/`builder().build()`; assert exact `to_string()` for one of each variant -- matrix coverage.
- `crates/noctis-core/src/registry/tests.rs`, `app/tests.rs`, `pipeline/tests/*.rs`, `crates/noctis-testkit/tests/middleware.rs` (+ any test the suite flags) -- migrate nested ids to full paths -- the new rules apply to existing fixtures.
- `crates/noctis-core/src/app/tests.rs` (or `plugin` tests) -- T13 (two Apps) and T14 (plugin-mounted duplicate) -- uniqueness is per App and covers plugins.
- `crates/noctis-core/src/observe/tests.rs` -- assert no `METRICS` row has a `module` label -- AD-29 boundary.
- `crates/noctis-testkit/tests/module_tree.rs` (new) -- end to end with real derives through the facade: tree `ops` → `ops/mod` → `ops/mod/audit`; each level has a before- and an after-Middleware appending to a shared log; the leaf adds a `#[derive(SlashCommand)]` whose handler records and returns `Err`, and a `#[derive(EventHandler)]` for `MESSAGE_DELETE` that returns `Err`; assert the Middleware order, `ErrorReport.module == Some("ops/mod/audit")` for the Command and Event origins, and that captured log output for the `noctis.interaction` and `noctis.event` spans contains `ops/mod/audit`; plus `App::build` of two roots `ops` returns `BuildViolation::DuplicateModule { id: "ops" }` -- outermost-surface proof of the ACs.
- `examples/project-template/src/modules/profile/mod.rs` -- move `.add::<handlers::Badge>()` into `.module(Module::new("profile/badges").add::<handlers::Badge>())` with a one-line comment on the id rule -- the template shows a tree.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md` -- §1: example with `.module(Module::new("profile/badges") ..)`, a "Module trees" subsection (grammar, root = one segment, child = parent + one segment, unique per App, `InvalidModuleId`/`ModuleIdError` variants/`DuplicateModule`, Middleware parent before child at every depth, `module` span field and `ErrorReport.module` carry the full path, no metric label), drop the `[planned]` id bullet -- Usage Skill documents trees.
- `examples/project-template/.claude/skills/noctis/references/troubleshooting.md` -- add rows for `invalid Module id …` and `Module id … is mounted twice` -- fix lookup.
- `examples/project-template/AGENTS.md` -- one bullet: Module ids are full paths (`profile/badges` inside `profile`), unique per App, fix `InvalidModuleId`/`DuplicateModule` by its reason -- agent guidance.
- `xtask/public-api.txt`, `examples/project-template/.claude/skills/noctis/SKILL.md` -- re-bless and set `API hash` to the printed hash -- AGENTS.md policy.

**Acceptance Criteria:**
- Given a facade App with the 3-level tree `ops` → `ops/mod` → `ops/mod/audit`, when it is built and a slash command and a `MESSAGE_DELETE` reach the leaf's handlers, then both run, before-Middleware run `ops`, `ops/mod`, `ops/mod/audit`, after-Middleware run in reverse, and each failure's `ErrorReport.module` and span `module` field is `ops/mod/audit`.
- Given two mounted Modules with the same id, when `App::build` runs, then it returns a `BuildError` containing `BuildViolation::DuplicateModule` naming the id, and every other violation is still listed.
- Given a mounted Module whose id breaks the grammar or its tree position, when `App::build` (or `--publish --dry-run`) runs, then the `BuildError` contains `InvalidModuleId` with the id and the matching `ModuleIdError`.
- Given the metric catalog, when it is inspected, then no metric has a `module` label.
- Given the changed public API, when the story is done, then the Usage Skill documents Module trees, the template's profile Module nests `profile/badges`, `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

### 2026-10-24 — Review pass
- verdicts: 31 findings — high 0, medium 9, low 14, false 8, maybe-false 0
- findings:
  - `[false]` `[reject]` (blind) NotUnderParent for a child of a grammar-invalid parent gives advice that is itself invalid — matrix row T8 in the intent contract settles exactly this output.
  - `[false]` `[reject]` (blind) a repeated invalid id yields one InvalidModuleId per occurrence plus DuplicateModule — matrix row T12 settles this interleaving.
  - `[low]` `[reject]` (blind) DuplicateModule names no location of the first occurrence — real, but the variant shape and exact Display text are fixed by the intent contract; the fix is a spec edit plus new public surface.
  - `[low]` `[reject]` (blind) "mounted twice" text for three occurrences — the exact Display string is pinned by the intent contract; fix is a spec edit.
  - `[low]` `[patch]` (blind) TooLong hard-codes 64 in its `#[error]` text while check_id uses MAX_ID_LEN — interpolate the constant so the text cannot drift; same rendered string.
  - `[false]` `[reject]` (blind) length checked before characters (65 x `ü` gives TooLong) — matrix row G7 settles this order deliberately.
  - `[low]` `[patch]` (blind) module_tree span assertion only checks substrings, not the `module` field — the test is weaker than the AC "span `module` field is ops/mod/audit"; match the rendered `module=` field.
  - `[low]` `[reject]` (blind) no test for a plugin-mounted multi-segment root or for dry-run reporting — plugin roots go into the same `self.modules` and dry run calls the same `Registry::fold` (lib.rs:518), already covered by T14 and the fold unit tests; extra tests add no defect fix.
  - `[medium]` `[defer]` (blind) epic-last.py keeps epics with maintainer-gated stories open forever and ignores git failures — pre-existing tooling from commit 2defad7; deferred.
  - `[medium]` `[defer]` (blind) AGENTS.md contradicts itself on cargo hack and on debug builds; gates.sh dropped the debug-dir check — pre-existing commit 2defad7 and fix edits agent-context files; deferred (grouped with the debug-build policy entry).
  - `[false]` `[reject]` (blind) the diff bundles unrelated tooling and policy changes — those are separate earlier commits (2defad7 and before) that appear only because the baseline predates them; the story commit 3417063 touches only story files plus a test-isolation fix in session_resume.rs needed by the new capturing-subscriber test.
  - `[low]` `[defer]` (blind) bg.sh cleanup race between two attached callers and log truncation — pre-existing tooling from 2defad7; deferred.
  - `[false]` `[reject]` (blind) `tracing.workspace = true` out of alphabetical order and needs a comment — the list already has `tracing-subscriber` after `twilight-gateway`, and session_resume.rs's KEEPER doc comment explains why the dependency exists.
  - `[low]` `[reject]` (blind) template move of `/badge` into `profile/badges` lacks guidance on when to nest — the move is what the spec's task asked for; the skill's root `BadgesList` is a subcommand sample, not a conflict; documentation nuance, not a defect.
  - `[low]` `[defer]` (edge) bg.sh second waiter exits with `exit ""` after the first moves the job dir — pre-existing tooling; deferred with the bg.sh entry.
  - `[low]` `[defer]` (edge) bg.sh waits forever when the detached job is killed — pre-existing tooling; deferred with the bg.sh entry.
  - `[low]` `[defer]` (edge) bg.sh rerun after a tree-changing command starts a second run — pre-existing tooling; deferred with the bg.sh entry.
  - `[medium]` `[defer]` (edge) gates.sh treats an epic-last.py crash as "not last" and prints ALL-OK — pre-existing tooling; deferred.
  - `[medium]` `[defer]` (edge) epic-last.py ignores failed git fetch/show — pre-existing tooling; deferred with the epic-last entry.
  - `[medium]` `[defer]` (edge) epic-last.py picks any in-progress/review row across epics — pre-existing tooling; deferred with the gates.sh entry.
  - `[false]` `[reject]` (edge) NotUnderParent suggests an invalid `A/<name>` under an invalid parent — matrix row T8 settles this.
  - `[low]` `[patch]` (edge) skill says id violations come "before the other build checks" — false for AppBuilder::build, which pushes setter/plugin violations (DuplicateState, InvalidShardTotal, ...) before the fold's; reword to "first among the Registry checks".
  - `[medium]` `[patch]` (vgap) the template's new `profile` -> `profile/badges` tree is never built by any gate — run the template's own tests once for this story and fix any failure.
  - `[medium]` `[defer]` (vgap) gates.sh no longer fails on target/debug — pre-existing commit 2defad7 policy change; deferred with the debug-build policy entry.
  - `[medium]` `[defer]` (vgap) epic-last.py untested — pre-existing tooling; deferred as filed.
  - `[low]` `[patch]` (vgap other) skill's "before the other build checks" claim — same root cause as the edge-layer row; same fix.
  - `[low]` `[patch]` (vgap other) AppBuilder::build `# Errors` doc lists the checks' order without the new Module-id violations — add InvalidModuleId/DuplicateModule as first among the checks.
  - `[false]` `[reject]` (intent D1) author-written path (R2) instead of composed path (R1) — the intent contract's Approach and Never clause ("no id rewriting at `.module(..)`") select R2 deliberately.
  - `[false]` `[reject]` (intent D2) grammar, 64-char cap and one-segment roots go beyond "path-like, unique" — the intent contract's Always clauses set exactly these rules.
  - `[low]` `[patch]` (intent D3) AC3 span proof only checks substrings — grouped with the blind span-assertion row; same fix.
  - `[medium]` `[defer]` (intent D4) out-of-scope AGENTS.md debug-build policy contradicts maintainer-decisions section 6 — pre-existing commit 2defad7 and fix edits agent-context/decision files; deferred to the maintainer.

## Design Notes

- (decision) Ids are author-written full paths, validated, not relative segments joined at nest time. Joining would change `Module::id()` after nesting and force owned ids through `BuildViolation`'s public `&'static str` fields and every registry entry; the spine (`Module::new("a/b")`) and the existing doc example (`Module::new("profile").module(Module::new("profile/badges"))`) already use full paths. Cost: a reusable library Module cannot be re-parented without changing its id; it exposes a root-level Module with its own single segment instead.
- (decision) Root ids are one segment and each child adds exactly one, so the id is the tree path (Story 13.3 can rely on the prefix relation). `.` and `:` are excluded so ids need no escaping in `<app>:<port>:` keys.
- (decision) Validation is at App build, not compile time: `Module::new` is called at run time, a panicking constructor would break startup safety, and position and uniqueness need the combined trees (maintainer decisions §4's middle tier).
- Middleware ordering and the `module` span/report fields already exist (Stories 5.6, 5.7, 7.6); this story proves them at depth 3 and does not rewrite them.

```rust
// Usage Skill §1 shape
Module::new("profile")
    .add::<Profile>()
    .module(Module::new("profile/badges").add::<Badge>())   // child = parent + "/" + one segment
```

## Verification

Run from `/home/claude/lanes/b/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace --all-features` -- expected: all pass, incl. new `module_tree` testkit test and the registry id tests
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, printed hash equals `SKILL.md`'s `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo hack test --release --workspace --each-feature` -- only in the epic's last story per the story loop (`tools/story-loop/gates.sh`); not required here
- Optional: `cargo check --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template` -- template compiles (delete `target/template` if disk is tight)

## Auto Run Result

- Summary: Module ids are author-written full tree paths (`profile`, `profile/badges`), validated at App build: grammar, 64-char cap (MAX_ID_LEN), one-segment roots, each child adds exactly one segment under its parent, and uniqueness across all trees. New BuildViolation variants InvalidModuleId and DuplicateModule are reported first among the Registry checks. Middleware order and the span/report `module` field proven at depth 3.
- Files changed: `crates/noctis-core/src/module.rs` (id grammar, check_id, tree fold), `crates/noctis-core/src/app.rs` (violations, `# Errors` doc order), `crates/noctis-macros/src/command.rs`, `crates/noctis/src/lib.rs`, existing tests updated to valid ids; `crates/noctis-testkit/tests/module_tree.rs` (new depth-3 test); `crates/noctis-testkit/tests/session_resume.rs` (subscriber isolation); template `profile/badges` nesting, AGENTS.md and noctis skill references; `xtask/public-api.txt` re-blessed with SKILL.md API hash.
- Review findings: 31 (high 0, medium 9, low 14, false 8). Patched 7 entries (medium 1, low 6): TooLong text interpolates MAX_ID_LEN; span assertion matches the `module=` field; skill wording "first among the Registry checks" (2 rows); `# Errors` doc lists module-id violations; template tests run for the new tree; AC3 span proof (grouped).
- Deferred 11: pre-existing story-loop tooling from commit 2defad7 (epic-last.py git-failure handling and untested, gates.sh crash-as-not-last, bg.sh races/hangs) and the AGENTS.md debug-build/cargo-hack policy contradiction (maintainer).
- Rejected 13: matrix rows T8/T12/G7 settle advice, interleaving and check order; DuplicateModule location and "mounted twice" text pinned by the intent contract; plugin-root/dry-run tests redundant (same fold); bundled-tooling diff is earlier commits; dependency order fine; template nesting guidance is a nuance; R2 path ids and grammar rules chosen by the intent contract.
- Follow-up review recommended: false (patched: high 0, medium 1, low 6).
- Verification: local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, xtask deps-check/tw-grammar/rules-catalog/api-snapshot/skill-check, tw_crosscheck, cargo deny, MSRV 1.91); cargo hack not run (not epic-last). CI paused by maintainer.
- Residual risks: reusable library Modules cannot be re-parented without changing their ids; deferred story-loop tooling issues remain.
