---
title: 'Story 13.2: Boot-time feature flags'
type: 'feature'
created: '2026-10-24'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/AGENTS.md'
  - '{project-root}/_bmad-output/implementation-artifacts/epic-13-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** One binary cannot run different feature sets: every mounted Module and registration is always registered, dispatched and published. FR-73/AD-7 require Boot Flags on Modules and single registrations, selected at startup (`--features a,b|all`, or config), filtered out of the descriptors before the Registry fold. Today `flags` is a "planned" argument of `#[command]`/menus and the Usage Skill marks Boot Flags `[planned]`.

**Approach:** Declare flags with `Module::flags([..])` and a `flags = [..]` argument on every kind attribute; pick the selection with a typed `BootFlags` value (`AppBuilder::boot_flags`, `NOCTIS_FEATURES`, `--features`); `Registry::fold` checks the full mounted trees (ids, flags) and then folds only the selected descriptors, so dispatch, intents and Publish (dry run included) see the subset. `App::from_env()` reads `--features` from the process command line, because the selection must exist before `build()`.

## Boundaries & Constraints

**Always:**
- Flag grammar (shared by declarations, CLI and env): 1..=32 characters of `a-z`, `0-9`, `_`, `-`; `all` and `none` are reserved (never declared, never in a list). Character indexes, like `ModuleIdError`.
- Selection: `BootFlags` is `all` (default, no source set), `none`, or a set of valid flags. Text form `all` | `none` | `a,b,...` (single commas, no spaces, no empty items, no duplicates). Display is canonical: `all`, `none`, or the set sorted and comma-joined. Equality is set equality.
- Semantics: an unflagged Module/registration always exists. A flagged one exists when the selection is `all` or contains **any** of its flags. Nesting is AND: a pruned Module drops its whole subtree (registrations, children, Middleware, `uses`, `base`/`group` declarations). `none` keeps only unflagged items.
- Selected registrations keep their fold order; Module ids (13.1) and declared flags are validated on the **full** mounted trees (plugins included), whatever is selected.
- A `base`/`group` declaration of a kept Module is dropped when the node had at least one Command before filtering and none after; a declaration that had none before still fails as today (REG-04).
- Locale catalogs and image Templates of pruned Modules stay merged/registered (App-wide data, memlog 11.2 decision); only registrations are filtered.
- Precedence: `--features` on the process command line > `NOCTIS_FEATURES`, both read by `App::from_env()` (trimmed; empty = unset); setters called afterwards win (existing `from_env` convention); default `all`.
- `App::run_with_args` (and `run`) with `--features S` where S ≠ the App's built selection fails with `CliError::BootFlagsMismatch` before anything else; equal S is accepted. `--features` combines with every mode, `--dry-run` and `--no-resume`.
- A selected flag (not `all`/`none`) that no mounted Module or registration (full trees) declares is a build violation (typo guard).
- Macro flag errors are compile errors (maintainer decisions §4); `Module::flags` errors are build violations. No new rule id in `docs/discord-rules.md` (framework convention).
- Every public API change updates `examples/project-template/` (code, `AGENTS.md`, Usage Skill), re-blesses `xtask/public-api.txt` and the Skill's `API hash` line in the same commit.

**Never:**
- No post-fold masking or stage-3 runtime Boot Flag check (filter before fold, AD-7); no `arc-swap` (13.3 only); no runtime enable/disable, rate limits or hooks.
- No re-fold inside `run_with_args`; no reading `std::env::args` in `AppBuilder::build` or `App::builder`.
- No flags on `#[autocomplete]` (it registers with its command), `LinkButton`, Middleware, Templates or locale catalogs.
- No change to `custom_id` format, Publish JSON shape or REG-05 accounting beyond fewer Commands.

## I/O & Edge-Case Matrix

Flag grammar (`noctis_rules::boot_flag::check`, used by macros, `Module::flags` checks and selection parsing):

| # | Flag | Expected |
|---|------|----------|
| F1 | `beta`, `a`, `0`, `_`, `-`, `eco_v2-x` | Ok |
| F2 | 32 × `a` | Ok |
| F3 | 33 × `a` | `TooLong { len: 33 }` |
| F4 | 33 × `ü` | `TooLong { len: 33 }` (char count, checked before chars) |
| F5 | `""` | `Empty` |
| F6 | `Beta` | `InvalidChar { index: 0, ch: 'B' }` |
| F7 | `a b` / `a.b` / `a/b` / `a:b` | `InvalidChar { index: 1, .. }` with ` ` / `.` / `/` / `:` |
| F8 | `ü`, `🙂x` | `InvalidChar { index: 0, ch }` |
| F9 | `all`, `none` | `Reserved { flag: "all" \| "none" }` |
| F10 | `ALL`, `alls`, `none_` | `InvalidChar{0,'A'}` / Ok / Ok |

Selection text (`BootFlags::from_str`; CLI value and `NOCTIS_FEATURES`; `index` counts chars in the whole value):

| # | Value | Expected |
|---|-------|----------|
| S1 | `all` | `BootFlags::all()` |
| S2 | `none` | `BootFlags::none()` |
| S3 | `a` / `b,a` | only {a} / only {a,b}, displays `a` / `a,b` |
| S4 | `""` | `Empty` |
| S5 | `a,`, `,a`, `a,,b` | `Empty` |
| S6 | `a,a` | `Duplicate` |
| S7 | `all,a`, `a,none`, `all,all` | `Reserved { flag }` of the first reserved item |
| S8 | `a,B` | `InvalidChar { index: 2, ch: 'B' }` |
| S9 | `a, b` | `InvalidChar { index: 2, ch: ' ' }` |
| S10 | 32-char item / 33-char item | Ok / `TooLong { len: 33 }` |
| S11 | `a,ü` | `InvalidChar { index: 2, ch: 'ü' }` |
| S12 | first failing item wins left to right: `B,,` | `InvalidChar{0,'B'}` |

`BootFlags::only(iter)`: same item rules (`only([])` == `none()`; `only(["all"])` = `Reserved`; `only(["a","a"])` = `Duplicate`).

CLI (`NoctisArgs::parse` / `split`):

| # | Args | Expected |
|---|------|----------|
| C1 | `--features a,b` / `--features=a,b` | `features() == Some(only{a,b})` |
| C2 | `--features all` / `none` | `Some(all)` / `Some(none)` |
| C3 | no `--features` | `features() == None` |
| C4 | `--features` (last) / `--features --dry-run` | `CliError::MissingFeatures` |
| C5 | `--features=` | `InvalidFeatures { value: "", reason: Empty }` |
| C6 | `--features a --features b` | `DuplicateFlag { flag: "--features" }` |
| C7 | `--features A` | `InvalidFeatures { value: "A", reason: InvalidChar{0,'A'} }` |
| C8 | `--publish --dry-run --features a`, `--no-resume --features a`, `--publish-clear --guild 1 --features a` | Ok |
| C9 | round trip: `parse(args.clone()) == args`; canonical order mode, `--dry-run`, `--no-resume`, `--features <display>` | equal |
| C10 | `split(["bot","--x","--features","a","--","--features","b"])` | features {a}; rest `["bot","--x","--","--features","b"]` |
| C11 | non-UTF-8 value after `--features` | `InvalidFeatures` with the lossy value |

Env / build / run:

| # | State | Expected |
|---|-------|----------|
| E1 | `NOCTIS_FEATURES=" a,b "` | builder selection {a,b} |
| E2 | `NOCTIS_FEATURES=""` | unset → `all` |
| E3 | `NOCTIS_FEATURES="a,,b"` | `EnvError::InvalidBootFlags { var, reason: Empty }` |
| E4 | env `a`, argv `--features b` | {b} (argv wins) |
| E5 | `from_env` then `.boot_flags(none())` | none (setter wins) |
| B1 | Module `eco` flags [economy]; selection {economy} / {beta} / all / none | kept / pruned / kept / pruned |
| B2 | Module flags [a,b], selection {b} | kept (any-of) |
| B3 | parent flags [a] pruned, child unflagged | child pruned |
| B4 | parent kept, child flags [b] not selected | child subtree pruned, parent kept |
| B5 | `#[command("beta stats", flags = ["beta"])]` in unflagged Module, selection none | command absent from dispatch and Publish JSON |
| B6 | flagged `#[event(flags = ["beta"])]` MESSAGE_DELETE, unselected | not run; its intents not requested by `App::intents()` |
| B7 | flagged component/modal, unselected | Route not in registry (unknown-route path); a kept Module `uses::<it>()` → existing CID-04 `UnmountedRoute` |
| B8 | `.base("beta", ..)` on kept Module, all `/beta *` Commands flagged off | declaration dropped, no REG-04 |
| B9 | `.base("ghost", ..)` with no Command ever | REG-04 as today |
| B10 | `Module::flags(["Beta"])` | `BuildViolation::InvalidBootFlag { module, flag: "Beta", reason: InvalidChar{0,'B'} }` |
| B11 | `Module::flags(["a","a"])` or `.flags(["a"]).flags(["a"])` | `InvalidBootFlag { reason: Duplicate }` once |
| B12 | `Module::flags([])` | no-op, unflagged |
| B13 | selection {ecnomy} while declared are {economy, beta} | `UnknownBootFlag { flag: "ecnomy", declared: ["beta","economy"] }` |
| B14 | selection {x}, nothing declared | `UnknownBootFlag { flag: "x", declared: [] }` |
| B15 | flag declared only inside a pruned subtree | counts as declared (full trees) |
| B16 | duplicate Module id only inside a pruned subtree | still `DuplicateModule` |
| B17 | two Modules flagged [a] and [b] each registering `/shop`, selection {a} | builds; one `/shop` |
| B18 | same with selection all | existing duplicate-command violation |
| R1 | App built with {a}, `run_with_args(["--features","a"])` | proceeds |
| R2 | built with all (`App::builder()`), `run_with_args(["--publish","--dry-run","--features","a"])` | `Error::Cli(BootFlagsMismatch { given: {a}, built: all })`, nothing sent |
| R3 | `from_env`-style build with argv `--publish --dry-run --features a` | printed JSON and diff contain only selected + unflagged Commands; the diff lists an unselected remote Command as `- delete …` |

Macro compile errors (`#[command]`, `#[user_command]`, `#[message_command]`, `#[event]`, `#[button]`, the five select attributes, `#[modal]`; component/modal kinds may keep their attribute's `[CID-03]` prefix):

| # | Input | Error text contains |
|---|-------|---------------------|
| M1 | `flags = ["beta", "eco"]` | compiles; descriptor carries both |
| M2 | `flags = []` | ``needs at least one Boot Flag: write `flags = ["beta"]` or remove it`` |
| M3 | `flags = "beta"`, `flags = [beta]`, `flags = [1]`, bare `flags` | ``takes a list of string literals: write `flags = ["beta"]` `` |
| M4 | `flags = ["Beta"]` | ``invalid Boot Flag `Beta` in `flags`: `B` at index 0 is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-` `` |
| M5 | `flags = ["all"]` | ``invalid Boot Flag `all` in `flags`: `all` is reserved`` |
| M6 | `flags = ["a", "a"]` | ``invalid Boot Flag `a` in `flags`: a Boot Flag is given twice: keep one`` |
| M7 | `flags` twice | the attribute's existing duplicate-argument error |
| M8 | 33-char flag | ``the Boot Flag has 33 characters: shorten it to at most 32`` |

Display strings (exact, tested):
- `BootFlagError::Empty`: ``a Boot Flag is empty: write flags such as `beta`, separated by single commas``
- `TooLong`: `the Boot Flag has {len} characters: shorten it to at most 32` (interpolate `MAX_LEN`)
- `InvalidChar`: ```{ch}` at index {index} is not allowed in a Boot Flag: use only a-z, 0-9, `_` and `-` ``
- `Reserved`: ```{flag}` is reserved: write `all` or `none` alone, never in a list or as a declared flag``
- `Duplicate`: `a Boot Flag is given twice: keep one`
- `CliError::InvalidFeatures`: ```--features` got `{value}`: {reason}``
- `CliError::MissingFeatures`: ```--features` needs a value: write `--features a,b`, `--features all` or `--features none` ``
- `CliError::BootFlagsMismatch`: ```--features {given}` does not match the Boot Flags this App was built with (`{built}`): build it with `App::from_env()`, which reads `--features`, or pass the same selection to `AppBuilder::boot_flags` ``
- `CliError::UnknownFlag`: existing text with the list ending ``--dry-run, --no-resume and --features <a,b|all|none>``
- `EnvError::InvalidBootFlags`: ``environment variable `{var}` is not a Boot Flag selection: {reason}``
- `BuildViolation::InvalidBootFlag`: ``invalid Boot Flag `{flag}` on Module `{module}`: {reason}``
- `BuildViolation::UnknownBootFlag`: ``Boot Flag `{flag}` is selected, but no mounted Module or registration declares it: declared flags are {list}`` where list is `` `a`, `b` `` (sorted, deduped) or `none`

</intent-contract>

## Code Map

- `crates/noctis-rules/src/lib.rs` L35-46 -- add `pub mod boot_flag;` (+ crate-doc bullet: framework grammar, no rule id). New `crates/noctis-rules/src/boot_flag.rs`: `pub const MAX_LEN: usize = 32`, `pub enum BootFlagError` (`Debug, Clone, Copy, PartialEq, Eq, thiserror::Error`, `#[non_exhaustive]`: `Empty`, `TooLong { len }`, `InvalidChar { index, ch }`, `Reserved { flag: &'static str }`, `Duplicate`), `pub fn check(flag: &str) -> Result<(), BootFlagError>`. Template: `crates/noctis-core/src/module.rs` L692 `check_id` / `ModuleIdError`.
- `crates/noctis-core/src/boot_flags.rs` (new) -- `pub struct BootFlags` (opaque; `Debug, Clone, PartialEq, Eq, Default`=all): `all()`, `none()`, `only(impl IntoIterator<Item: AsRef<str>>) -> Result<Self, BootFlagError>`, `is_all()`, `selects(&[&str]) -> bool` (empty slice = true), `FromStr` (matrix S*), `Display`. Re-export `BootFlags`, `BootFlagError` at `crates/noctis-core/src/lib.rs` root (~L231/268) and in the facade `crates/noctis/src/lib.rs` (~L592 list; prelude unchanged).
- `crates/noctis-core/src/module.rs` -- `Module` (L158) gains module-level flags and per-registration flags; `pub fn flags<I: IntoIterator<Item = &'static str>>(self, I) -> Self` (`#[must_use]`, appends); hidden `pub fn __add_flagged(self, flags: &'static [&'static str], d: impl Descriptor) -> Self` beside `__add` (L224); a `pub(crate)` pruning step returning the selected clone of a tree (or `None`), preserving order, applying the base/group drop rule (`NodeDecl`, L88; Command paths from `CommandHandler`'s `paths`/pattern in `crates/noctis-core/src/command.rs` L432); `Debug` (L520) shows flags. Update the type rustdoc (add a "# Boot Flags" section).
- `crates/noctis-core/src/registry.rs` L153 `fold(roots, requested, strings)` -- add a `flags: &BootFlags` parameter: run `check_module_ids` (L323) and the new flag checks (InvalidBootFlag in pre-order, then UnknownBootFlag sorted) on the **full** trees, then fold the pruned trees; update the fold rustdoc's violation order. Callers: `crates/noctis-core/src/app.rs` L2631 (`build`) and `crates/noctis-core/src/lib.rs` L558 (`route_table`, pass `&BootFlags::all()`), plus test callers.
- `crates/noctis-core/src/error.rs` -- `BuildViolation` (~L216, `#[non_exhaustive]`): `InvalidBootFlag { module: &'static str, flag: &'static str, reason: BootFlagError }`, `UnknownBootFlag { flag: String, declared: Vec<&'static str> }`, each with a doctest asserting `to_string()`; `EnvError` (L962): `InvalidBootFlags { var: &'static str, reason: BootFlagError }`.
- `crates/noctis-core/src/cli.rs` -- add `const FEATURES: &str = "--features"`, `Parser.features`, value via `--features=v` or `take_value` (L292), `NoctisArgs::features() -> Option<&BootFlags>`, `IntoIterator` emits `--features <display>` last; `CliError::{InvalidFeatures { value: String, reason }, MissingFeatures, BootFlagsMismatch { given: BootFlags, built: BootFlags }}`; update module docs, `UnknownFlag` text (L93), `split` reserved-name doc (L343). Tests in `crates/noctis-core/src/cli/tests.rs`.
- `crates/noctis-core/src/app.rs` -- `AppBuilder` field + `pub fn boot_flags(&mut self, BootFlags) -> &mut Self` (last call wins; rustdoc with semantics); `App` field + `pub fn boot_flags(&self) -> &BootFlags`; `App::from_env` (L241, docs L206) reads `NOCTIS_FEATURES` in `from_lookup` (L1705, after `NOCTIS_DEV_GUILD`) and then `--features` via `NoctisArgs::split(std::env::args_os())` (a malformed Noctis flag → `Error::Cli`): add a `pub(crate) fn from_sources(lookup, args) -> Result<AppBuilder, Error>` used by `from_env`, keep `from_lookup` for existing tests; `run_with_args` (L729) checks `BootFlagsMismatch` right after parsing; `build()` (L2504) passes the selection to `Registry::fold`; update `build` `# Errors` order and `run_with_args` docs.
- `crates/noctis-macros/src/kind.rs` L198/L224 -- `scaffolding`/`items` take the parsed flags; `__register` emits `module.__add_flagged(&[..], descriptor)` when non-empty, else `__add`; `fallback` passes none.
- New `crates/noctis-macros/src/flags.rs` -- shared `flags = [..]` value parser (M2-M8, using `noctis_rules::boot_flag::check`). Wire into: `crates/noctis-macros/src/command.rs` (remove `flags` from `PLANNED` L33, accept in the arg match ~L239, update `EXPECTED` L35 and the planned test ~L1223), `crates/noctis-macros/src/menu.rs` (L55, ~L120, `EXPECTED` L57), `crates/noctis-macros/src/event.rs` (`parse_args` L17: today only `ordered`/`droppable` idents; its error text), `crates/noctis-macros/src/route.rs` (allowed-name list ~L425, `Seen` L238, `parse_arg` ~L470, `ARGS_HELP`) which serves components and `#[modal]` (`component.rs` L129, `modal.rs` L1381 call `scaffolding`).
- Tests to copy patterns from: `crates/noctis-core/src/app/tests.rs` L3055 (`run_mode_to` capturing dry-run output), `crates/noctis-testkit/tests/publish.rs` (FakeDiscordHttp, `run_with_args(["--publish","--dry-run"])` L397), `crates/noctis-testkit/tests/module_tree.rs` (facade derives + FakeGateway dispatch), macro unit tests in `route.rs`/`command.rs`, trybuild suites under `crates/noctis/tests/ui` (or wherever `compile_fail` cases live; `compile_fail` only).
- Template: `examples/project-template/src/modules/profile/mod.rs` (`profile/badges` child from 13.1), `src/main.rs` L37-41 comment (`[planned] --features`), `README.md` L109 row, `.env.example` (add commented `# NOCTIS_FEATURES=`), `AGENTS.md`, `tests/flows.rs`; skill `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md` §1 L27 and §2 L110-120, `SKILL.md` L23 (`API hash`) and L476 row, `references/troubleshooting.md` build-error table.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/boot_flag.rs`, `crates/noctis-rules/src/lib.rs` -- grammar, `BootFlagError`, `MAX_LEN`; table tests F1-F10 and exact Display -- one grammar for macros and runtime.
- `crates/noctis-core/src/boot_flags.rs` (+ `lib.rs`, `crates/noctis/src/lib.rs` re-exports) -- `BootFlags` with `FromStr`/`Display`/`only`/`selects`; tests S1-S12 and `only` rows -- parse, don't validate.
- `crates/noctis-core/src/error.rs` -- `InvalidBootFlag`, `UnknownBootFlag`, `EnvError::InvalidBootFlags` with doctests -- typed failures.
- `crates/noctis-core/src/module.rs` -- `Module::flags`, `__add_flagged`, pruning with the base/group drop rule, docs -- descriptors carry flags.
- `crates/noctis-core/src/registry.rs` (+ `lib.rs` `route_table`, existing fold test callers) -- selection parameter, full-tree checks, fold of pruned trees; unit tests B1-B4, B8-B18 via `Registry::fold`/`builder().build()` -- filter before fold.
- `crates/noctis-core/src/cli.rs`, `cli/tests.rs` -- `--features`, new `CliError` variants, canonical rendering; tests C1-C11 and updated `UnknownFlag` text -- joins the Story 6.9 parser.
- `crates/noctis-core/src/app.rs`, `app/tests.rs` -- `AppBuilder::boot_flags`, `App::boot_flags`, `from_env`/`from_sources` (E1-E5 through `from_sources`), mismatch check (R1, R2), and R3: `from_sources(lookup, ["bot","--publish","--dry-run","--features","a"])` → `build()` → `run_mode_to` against a fake remote holding an unselected Command, asserting the JSON and diff -- AC2 at the CLI surface.
- `crates/noctis-macros/src/flags.rs`, `kind.rs`, `command.rs`, `menu.rs`, `event.rs`, `route.rs` -- `flags = [..]` on every kind attribute; unit tests M1-M8 per attribute family; one trybuild `compile_fail` case for M4 through the facade -- compile-time first.
- `crates/noctis-testkit/tests/boot_flags.rs` (new) -- facade App: unflagged Module `core` (`/ping`, `#[command("beta stats", flags = ["beta"])]`, `#[event(flags = ["beta"])]` MESSAGE_DELETE), and a `#[button("core.beta", flags = ["beta"], defaults(..))]`; Module `eco` `.flags(["economy"])` (`/balance` and an unflagged button `eco.buy`); built with `boot_flags(only{economy})`: through FakeGateway `/balance`, `/ping` and a click on `eco.buy` run their handlers, while `/beta stats`, a click on `core.beta` and MESSAGE_DELETE run none; `publish(..).dry_run(true)` JSON lists `ping`, `balance` only; `run_with_args(["--publish","--dry-run","--features","economy"])` is Ok and `["--features","beta"]` is `BootFlagsMismatch` -- outermost-surface proof of AC1.
- `examples/project-template/src/modules/profile/mod.rs`, `tests/flows.rs` -- `Module::new("profile/badges").flags(["badges"])`; test: `boot_flags(none())` build has no `/badge`, default has it -- template shows Boot Flags.
- `examples/project-template/src/main.rs`, `README.md`, `.env.example`, `AGENTS.md` -- replace `[planned]` text; one AGENTS bullet (flags grammar, any-of, `--features` read by `App::from_env`, mismatch error) -- operator docs.
- `examples/project-template/.claude/skills/noctis/references/modules-middleware-ratelimits.md`, `SKILL.md`, `references/troubleshooting.md` -- §2 Boot Flags: declarations (`Module::flags`, `flags = [..]` on each kind), grammar, any-of/AND, `all`/`none`, sources and precedence, `BootFlags` API, mismatch rule, what is filtered (registrations, `uses`, emptied `base`/`group`) vs kept (catalogs, Templates), Publish of a subset removes unselected remote Commands, multi-App note (`boot_flags` per App + `run_with_args`); drop `[planned]` in §1; troubleshooting rows for every new error text -- Usage Skill documents Boot Flags.
- `xtask/public-api.txt`, `SKILL.md` -- re-bless, set `API hash` -- AGENTS.md policy.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- already carries the planning decisions; append implementation decisions if any.

**Acceptance Criteria:**
- Given Modules and registrations carrying Boot Flags, when the App is built with `--features a,b` (via `App::from_env`), `all`, `none` or `AppBuilder::boot_flags`, then unselected registrations are absent from dispatch, requested intents and Publish, and selected ones behave exactly as before.
- Given `--publish --dry-run --features a`, when the binary's `App::from_env()…build()?.run()` path runs it, then the printed JSON and diff contain only Commands of selected or unflagged registrations, and nothing is written.
- Given an App whose built selection differs from `--features` on its command line, when `run_with_args` runs, then it returns `CliError::BootFlagsMismatch` and sends no request.
- Given the new public API, when the story is done, then the Usage Skill documents Boot Flags, the template flags `profile/badges`, and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- (decision) The selection must exist before `build()`, because the fold of the full set can fail where a subset builds (two flagged alternatives of `/shop`, B17). `App::from_env()` (the "process environment" entry point the template uses) therefore reads `--features` from argv as well as `NOCTIS_FEATURES`; `run`/`run_with_args` keep parsing `--features` (spine Conventions) and only verify it matches. Rejected: re-folding inside `run_with_args` (would need to rebuild `AppShared`, intents and Renderer registration after build, and still fails on B18-style conflicts at build time).
- (decision) Unknown selected flags are build errors (typo guard). Multi-App processes give each App its own selection with `boot_flags` and run it with `run_with_args`.
- (decision) Any-of within one declaration, AND across nesting; `none` is reserved alongside `all` so an operator can run only the unflagged core.
- (decision) Catalogs and Templates stay App-wide when their Module is pruned (strings are looked up by key; a kept Module may render them). Resolves the memlog 11.2 open point.
- (decision) Flags on kind attributes (compile-checked), not on `Module::add` (keeps one registration style, §7); Module-level flags are runtime values, so they are build violations.
- Spine stage 3 "Boot Flag check" needs no runtime code: pruned registrations never reach the Registry.

```rust
Module::new("eco").flags(["economy"]).add::<Balance>();
#[derive(SlashCommand)]
#[command("beta stats", description = "Beta stats", flags = ["beta"])]
struct BetaStats;
// bot --features economy,beta | --features all | --features none ; NOCTIS_FEATURES=economy
```

## Verification

Run from `/home/claude/lanes/b/noctis-rs`, release only:

**Commands:**
- `cargo fmt --all` -- expected: no diff afterwards
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0
- `cargo test --release --workspace --all-features` -- expected: all pass, incl. `boot_flags` testkit test, cli/registry/app/macro tests
- `cargo test --release --manifest-path examples/project-template/Cargo.toml --target-dir target/template` -- expected: pass (delete `target/template` afterwards if disk is tight)
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- expected: exit 0, printed hash equals `SKILL.md`'s `API hash`
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- skill-check`, `-- tw-grammar --check` -- expected: exit 0
- `cargo test --release -p noctis-image --features render --test tw_crosscheck` -- expected: pass
- `cargo deny check` -- expected: exit 0
- `cargo +1.91 check --release --workspace` -- expected: exit 0
- `cargo hack` is not run here (only in an epic's last story, `tools/story-loop/gates.sh`)
