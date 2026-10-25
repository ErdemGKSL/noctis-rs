---
title: 'Story 14.5: Usage Skill completeness and agent trial'
type: 'chore'
created: '2026-10-25'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/examples/project-template/AGENTS.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** The Usage Skill (`examples/project-template/.claude/skills/noctis/`) is the only Noctis source a bot author's agent may read (FR-50, AD-17, maintainer decision §5), but it still says `Status: v0`, carries 28 `[planned]` markers and 8 internal `Epic N`/`Story N.M` references, has no troubleshooting index by rule id (73 of 135 catalog ids are absent from `troubleshooting.md`, 10 from `limits-cheatsheet.md`), no `Common mistakes` section, no concept map in its references, and component `**Limits:**` entries do not link `docs/discord-rules.md`. Nothing gates these properties, and SM-3 (UJ-5 agent trial, ≥ 4 of 5 compile) has never been measured.

**Approach:** Extend `xtask skill-check` with a completeness check (required references and headings, marker ban, rule-id index, cheatsheet coverage, Limits links, UD-7 scan of Markdown Rust examples), bring the skill to pass it, then run the UJ-5 trial five times with subagents on isolated copies of the Project Template and store the summary under `_bmad-output/implementation-artifacts/`.

## Boundaries & Constraints

**Always:**
- Every `[planned]` item is resolved against `xtask/public-api.txt` (and the macro tests): if it exists, drop the marker and make the text match the real signature; if not, delete the claim or say plainly "Noctis 0.1 does not provide X; do Y instead".
- New problem lines use the existing format `skill-check: <subject>: <message>`, sorted and deduplicated by `Problems`; subjects are repo-relative paths (`<file>:<line>` when a line is known).
- Rule ids come from `docs/discord-rules.md` table rows whose first cell starts with a bold id (`| **CMD-01** …`), outside fenced code, in document order — the same rows `rules_catalog::catalog::parse` reads (a unit test pins equality on the real catalog: 135 ids).
- Limits links use the canonical URL `https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md` with the section anchor (e.g. `#9-components-v2-messages`, `#10-modals`).
- Trial agents get only their copy of the Project Template; they learn Noctis only from its `AGENTS.md` and `.claude/skills/noctis/`. All trial rounds, including failed ones, are recorded.
- Keep `SKILL.md`'s `**API hash:**` line unchanged (no public API change in this story).

**Never:**
- No public API change (no Kv/`Id` guard; see D4), no `api-snapshot --bless`, no edits under `crates/`.
- No trial copy inside the repository (skill-check lints every crate under `examples/`); no new dependencies in trial copies; never run `cargo hack`.
- Do not weaken existing skill-check checks (mentions, hash, component entries, doctrine, recipes) or the base-fixture expectations they encode.
- Do not make the agent trial a CI or `gates.sh` gate (it needs an agent; D3).

## I/O & Edge-Case Matrix

`S` = skill dir `examples/project-template/.claude/skills/noctis`; `TS` = `S/references/troubleshooting.md`. Messages are exact.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Q1 required file | `S/references/patterns.md` absent (likewise `concept-map.md`, `limits-cheatsheet.md`, `troubleshooting.md`) | `S/references/patterns.md: required reference missing`; that file's later checks skipped | — |
| Q2 required heading | `patterns.md` lacks `## Common mistakes` (also `## Patterns`, `## Anti-patterns`; `concept-map.md`: `## tescord / dbi → Noctis`; TS: `## Rule id index`); heading only inside a ``` fence, or as `### Common mistakes` | `S/references/patterns.md: missing heading "## Common mistakes"` | exact level-2 text, trailing spaces trimmed |
| Q3 index completeness | a `references/x.md` file not named as code span `` `references/x.md` `` in `S/SKILL.md` | `S/SKILL.md: references/x.md is not listed in the reference index` | — |
| Q4 dangling index | `SKILL.md` names `` `references/gone.md` `` that does not exist | `S/SKILL.md: lists references/gone.md, which does not exist` | — |
| Q5 planned marker | any skill `.md` line with token `planned`, any case (`[planned]`, `**planned**`, `Planned (…)`), also inside fences | `<file>:<n>: "planned" marker; document what exists or say it is not provided` | `unplanned`, `planner` are other tokens: no problem |
| Q6 v0 marker | token `v0` (`Status: v0`) | `<file>:<n>: "v0" status marker` | `COLRv0` (one token), `v0.1` (followed by `.`+digit): no problem |
| Q7 planning ref | `Epic 7`, `Story 14.6` (capitalized word + space + number) | `<file>:<n>: internal planning reference "Epic 7"` | `epic 7`, `Epics` : no problem |
| Q8 index happy | TS `## Rule id index` table rows = all catalog ids, catalog order, 3+ non-empty cells, 3rd cell contains `.md` | no problem | id cell may be `CMD-01`, `` `CMD-01` `` or `**CMD-01**` |
| Q9 index missing id | catalog has `VC-02`, index lacks it | `TS: rule id index: VC-02 has no row` | order check skipped while sets differ |
| Q10 index unknown/invalid | row `ZZ-01` or `cmd-01`, `CV2-1`, `ŞŞ-01` (not a catalog id) | `TS: rule id index: ZZ-01 is not a catalog rule` (raw cell text for invalid ids) | header and `|---|` separator rows ignored |
| Q11 duplicate row | `CMD-01` twice | `TS: rule id index: CMD-01 listed twice` | — |
| Q12 order | same set, `CMD-02` before `CMD-01` | `TS: rule id index: rows are not in catalog order (first difference: CMD-02)` | only when no Q9-Q11 problem |
| Q13 short row | `| CMD-01 | |` or 2 cells | `TS: rule id index: CMD-01 row needs id, meaning and fix cells` | — |
| Q14 fix cell | 3rd cell `see above` | `TS: rule id index: CMD-01 fix cell must name a skill reference (*.md)` | — |
| Q15 empty index | heading present, no table rows | one Q9 line per catalog id | — |
| Q16 cheatsheet | `limits-cheatsheet.md` lacks token `OPT-14` (`[A-Za-z0-9_-]` run) | `S/references/limits-cheatsheet.md: OPT-14 is missing` | `XOPT-14`, `OPT-141` do not count |
| Q17 catalog unreadable / no ids | `docs/discord-rules.md` missing / no bold-id rows / an id twice | `docs/discord-rules.md: cannot read: …` / `docs/discord-rules.md: no rule rows found` / `docs/discord-rules.md: CMD-01 is listed twice`; Q8-Q16 skipped | — |
| Q18 Limits link | a due builder's entry: text from `**Limits:**` to the next `**Where:**`/`**Builder:**` or body end has no Markdown link target (`](…)`) containing `docs/discord-rules.md` | `<entry file>:<line>: component-entries: entry for Container: **Limits:** does not link docs/discord-rules.md` | link elsewhere in the entry, or bare text path: still a problem; relative or absolute target: ok |
| Q19 UD-7 in examples | a ```` ```rust ```` fence (info string's first word `rust`, e.g. `rust,ignore`, `rust recipe=x`) in a skill `.md` or `docs/**/*.md` with identifier `msg_collector`, `on_listener`, `wait_for` | `<file>:<n>: [UD-7] collectors are not allowed; one #[handler] per Route` (resp. `inline listener…`, `await-component loop…`, the doctrine's UD-7 texts) | identifiers in `//` comments and `"…"` literals ignored; ```` ```text ````/prose ignored |
| Q20 real repo | repository after this story | `skill-check: ok (<N> public items mentioned, <E> component entries checked, <F> example files linted, 6 recipes in sync, 135 rule ids indexed)` | — |

</intent-contract>

## Code Map

- `xtask/src/skill_check/mod.rs` -- module docs list the checks; `check()` (~l.146) runs them and builds the summary (append `, {indexed} rule ids indexed`); `Repo::{read, files}`; `SKILL_DIR`. Register `mod completeness;`.
- `xtask/src/skill_check/components.rs` -- `LABELS`, `headings()`, `check()` (~l.148): per-entry label loop is where Q18 goes (only for due builders with a single entry that has `**Limits:**`).
- `xtask/src/skill_check/doctrine.rs` -- `UD7_*` consts (l.41-44), `AWAIT_LOOPS` (l.54), `words()` (l.112): make them `pub(super)` and reuse for Q19 (same word rules: `collector(s)`, `listener(s)`, await-loop names).
- `xtask/src/skill_check/mention.rs` -- `read_skill` (all `*.md` below the skill, paths relative to root), `tokens()` helper pattern for Q5-Q7/Q16.
- `xtask/src/rules_catalog/mod.rs:56` `mod catalog;` -> `pub(crate) mod catalog;` (for the equality unit test); `is_rule_id` (l.91) for Q10.
- `xtask/tests/skill_check.rs` -- base fixture `root()` (~l.90), `OK` const (l.45), `real_repo_passes` (l.192, ends-with assertion), `limits.md` fixture used by m3/m8 tests.
- `xtask/src/main.rs` -- top docs and `USAGE` text for `skill-check`.
- Skill: `SKILL.md` (banner l.20-22; §5 anti-patterns l.412; §5b; §6 concept map l.462; §7 reference index l.484), `references/*.md` (15 files). `[planned]` sites: testing.md:133; components-v2.md:12,45,139,425,431,454,850,1070; i18n.md:329; modals.md:335,351; adapters.md:11,131,815; modules-middleware-ratelimits.md:22,669; app-and-sharding.md:6,146,1005,1162; routes-payloads-refs.md:110,814; commands.md:76,567,585,859,887,917 (`Ctx` trait: does not exist, decision 14.2 D2); responder.md:473,747; limits-cheatsheet.md:17. Planning refs: components-v2.md:139, i18n.md:206, modals.md:351, adapters.md:315,820 (keep the cluster-guide link, drop "(written with Story 14.6)"), modules-middleware-ratelimits.md:669, app-and-sharding.md:1162, responder.md:747.
- Snapshot facts for the triage: present — `select_options`, `AppBuilder::limits`, `State`, `AppBuilder::state`, `StaticMessage`, `noctis::ui::validate`, `resolved` accessors; absent — `try_string_select`, `button_with`, `AppBuilder::template`. Verify each by `grep` in `xtask/public-api.txt` and the skill's own signatures elsewhere.
- Coverage gaps today: cheatsheet lacks CTX-03, INT-14, META-02, META-03, META-04, META-05, OPT-02, OPT-14, TW-01, TW-02; troubleshooting lacks 73 ids (all now go in the index).
- `examples/project-template/Cargo.toml` -- `noctis = { path = "../../crates/noctis", … }` (trial copies rewrite it to `/home/claude/lanes/d/noctis-rs/crates/noctis`); `examples/project-template/target` (3.5 GB, reused as the trials' `CARGO_TARGET_DIR`; ~8.8 GB free).
- `.github/workflows-disabled/ci.yml` (`skill-check` job l.198) and `tools/story-loop/gates.sh:40` already run skill-check: no change.
- Continuity (14.4 done): `CHANGELOG.md` `## [Unreleased]` has subsections per story; 14.2 deferred to this story the skill's `[planned] Ctx` line and the Kv-stored `Id` migration sentence (`references/adapters.md` §3).

## Tasks & Acceptance

**Execution:**
- `xtask/src/skill_check/doctrine.rs`, `xtask/src/rules_catalog/mod.rs` -- widen visibility as in the Code Map; no behaviour change.
- `xtask/src/skill_check/completeness.rs` (new) -- `catalog_ids(text, problems) -> Vec<String>` and checks Q1-Q17, Q19 with consts `REQUIRED: &[(&str, &[&str])]` (file → required level-2 headings); `check(repo, skill_dir, files, problems) -> usize` (number of indexed ids). Unit tests: every matrix row Q1-Q17, Q19 on small strings, plus `real_catalog_ids_match_rules_catalog` (reads `../docs/discord-rules.md` via `CARGO_MANIFEST_DIR`, compares with `catalog::parse` row ids, asserts 135).
- `xtask/src/skill_check/components.rs` -- Q18 plus unit tests (link in Limits ok; bare path, link in Where only, no link: problem).
- `xtask/src/skill_check/mod.rs`, `xtask/src/main.rs` -- wire the check, extend docs, summary and `USAGE`.
- `xtask/tests/skill_check.rs` -- base fixture gains a minimal `docs/discord-rules.md` (two bold-id rows, e.g. REG-06 and CMD-01), `S/references/{limits-cheatsheet,troubleshooting,concept-map,patterns}.md` satisfying Q1-Q16 (move the `limits.md` content into `limits-cheatsheet.md`; m3/m8 tests follow), and a `SKILL.md` listing them; update `OK` and `real_repo_passes`; add CLI tests `q1_missing_reference`, `q5_planned_marker`, `q9_index_missing_id`, `q18_limits_without_link`, `q19_listener_in_markdown_example` with exact lines.
- Skill `SKILL.md` -- replace the v0 banner with a 0.1 status line (keep the API hash line byte-identical); §6 becomes a 3-line pointer to `references/concept-map.md`; §7 lists `concept-map.md` and `patterns.md`.
- Skill `references/concept-map.md` (new) -- `## tescord / dbi → Noctis` (move §6's table, add dbi rows) and `## serenity / poise → Noctis` (short).
- Skill `references/patterns.md` (new) -- `## Patterns` (≥ 10 named patterns, each: when, shape, short code, link to the full recipe), `## Anti-patterns` (link SKILL.md §5, add the ones it lacks), `## Common mistakes` (table symptom | cause | fix | rule id, ≥ 15 rows: Kv-stored `Id` reads `Malformed` (store `u64`; an already-stored value: treat as absent and overwrite), `app.run()` in tests, `ctx.update` in a command, `description = "…"` literal, missing `.add::<T>()`, `HandlerResult<()>` for a command, key missing in `en-US.yaml`, `auto_defer`/`Manual` mismatch, required option after `Option`, hand-built `custom_id`, `String` in an inline Payload, …; plus whatever the trials hit).
- Skill `references/troubleshooting.md` -- add `## Rule id index` (all 135 ids, catalog order, `| Rule | Meaning | Fix / where |`, fix cell names a reference); keep the phase tables.
- Skill `references/limits-cheatsheet.md` -- add the 10 missing ids; link the catalog URL.
- Skill `references/components-v2.md`, `references/modals.md` -- every due entry's `**Limits:**` links its catalog section.
- Skill, all files -- resolve every `[planned]`/`Planned`/planning reference per Always; add the 14.2-deferred `Id` migration sentence to `adapters.md` §3.
- `examples/project-template/AGENTS.md` -- point error lookup at the `Rule id index`, name `patterns.md` (common mistakes) and `concept-map.md`.
- Trial (after `skill-check` passes) -- per round r: `mkdir -p /home/claude/lanes/d/uj5-trials/round-r`; for n in 1..5 copy the template without `target/` (`rsync -a --exclude target examples/project-template/ /home/claude/lanes/d/uj5-trials/round-r/trial-n/`), rewrite the `noctis` path to `/home/claude/lanes/d/noctis-rs/crates/noctis`; launch 5 subagents in one message with the Design Notes prompt; then verify each yourself (Design Notes); if fewer than 4 compile, fix the skill from the failures (common mistakes, missing docs), rerun skill-check and start a new round with fresh copies; at most 3 rounds, then HALT `blocked`. Delete `/home/claude/lanes/d/uj5-trials/` at the end.
- `_bmad-output/implementation-artifacts/uj5-agent-trial-summary.md` (new) -- per round and trial: date, model, files written, files read (no Noctis source), first-attempt build result, build attempts, final build, clippy, first line of each compile error, skill gaps; round verdict; skill changes made between rounds.
- `CHANGELOG.md` -- `### Usage Skill (v1)` under `[Unreleased]`: completeness check, new references, markers removed, trial result.
- `_bmad-output/planning-artifacts/architecture/architecture-noctis-rs-2026-10-19/.memlog.md` -- D1-D5 were logged at planning; append a `(decision)` line only for a new decision made while implementing.

**Acceptance Criteria:**
- Given the repository after this story, when `cargo run --release -p xtask -- skill-check` runs, then it exits 0 with the Q20 line (every public item mentioned, every due builder's entry has Where/Limits/Builder with a catalog link).
- Given any skill file, when searched for the token `planned` (any case) or `v0` (not followed by `.<digit>`), or for `Epic <n>` / `Story <n>.<m>`, then nothing matches and SKILL.md has no v0 banner.
- Given `references/`, when listed, then `concept-map.md`, `limits-cheatsheet.md`, `troubleshooting.md` (with a complete `## Rule id index`) and `patterns.md` (`## Patterns`, `## Anti-patterns`, `## Common mistakes`) exist and SKILL.md §7 lists every reference file.
- Given a Rust example in the skill or `docs/`, when skill-check scans it, then no per-message listener, collector or await loop appears.
- Given the UJ-5 prompt run by 5 subagents on isolated template copies, when the implementer rebuilds each copy with `cargo build --release --locked`, then at least 4 of 5 in the final round build with a leaderboard Template, View and Command mounted and no Noctis source read, and `uj5-agent-trial-summary.md` records every round.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions:** D1 completeness is a skill-check gate (CI already runs it), checking structure and coverage, not prose quality. D2 the concept map and patterns/common mistakes live in new reference files; SKILL.md keeps its short anti-pattern table and best practices (always loaded) and points to them. D3 the agent trial is a one-off story deliverable, not a gate (it needs an agent and minutes per run); `xtask skill-check` guards the skill between trials. D4 the Kv/`Id` postcard guard (deferred by 14.2/14.4) stays out: no 14.5 AC needs it, it would be an additive post-0.1 API, and the trial measures compilation (the trap is runtime); the skill documents it as a common mistake. D5 trial success = the implementer's own `cargo build --release --locked` of the copy; first-attempt compile (UJ-5 wording) is reported, not gated.

**Trial prompt** (verbatim per subagent; `<DIR>` = its copy):

> You work only in `<DIR>`, a Discord bot built on the Noctis framework. Read `<DIR>/AGENTS.md` and learn Noctis only from `<DIR>/.claude/skills/noctis/` (and `<DIR>/.claude/skills/rust-best-practices/` for Rust). Never read Noctis source: nothing under `/home/claude/lanes/d/noctis-rs/` outside `<DIR>`, `~/.cargo/registry`, `~/.cargo/git`, or any `target/` directory. Do not add dependencies. Task: add a leaderboard feature in `src/modules/leaderboard/` and mount it in `src/lib.rs`: a Template (a rendered image of the top 10 members by points in the guild), a View (a plain render function showing that image in a Media Gallery with Refresh and Previous/Next page buttons), and a Command `/leaderboard` that opens the View, plus a separate Command `/points add user amount` (Manage Guild) that adds points stored with `ctx.kv()`. Every user-visible string goes in `locales/en-US.yaml` and `locales/tr.yaml`. After writing, run `CARGO_TARGET_DIR=/home/claude/lanes/d/noctis-rs/examples/project-template/target cargo build --release --locked` in `<DIR>` (timeout 600000) and record the result as the first attempt; fix and rebuild until it succeeds or 5 attempts are used; then run the same env with `cargo clippy --release --locked --all-targets -- -D warnings` once. Reply with: files created/changed; every file path you read; build attempts and each failing attempt's first error line; final build and clippy results; anything the skill did not explain.

**Verification per trial** (implementer): rerun the build command above (exit 0 = compiled); `grep -l "derive(Template" ` / `"derive(SlashCommand"` / `fn render_` under `src/modules/leaderboard/` and `leaderboard` in `src/lib.rs`; reject the trial (count it failed) if its read list names a forbidden path.

## Verification

**Commands:**
- `cargo fmt --all` -- no diff afterwards
- `cargo test --release -p xtask` -- all pass, including `skill_check::completeness::` and the new CLI tests
- `cargo run --release -p xtask -- skill-check` -- the Q20 line
- `grep -rniwE "planned|v0" examples/project-template/.claude/skills/noctis` and `grep -rnE "Epic [0-9]|Story [0-9]+\.[0-9]" examples/project-template/.claude/skills/noctis` -- only `v0.<digit>` hits, if any
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- api-snapshot`, `rules-catalog`, `deps-check`; `cargo deny check`; `cargo +1.91 check --release --workspace` -- pass unchanged
- Template gates (`tools/story-loop/gates.sh` lines 24-29 commands) -- pass (only `AGENTS.md` and skill text change)

**Manual checks:**
- `_bmad-output/implementation-artifacts/uj5-agent-trial-summary.md` shows ≥ 4/5 compiled in the final round; `/home/claude/lanes/d/uj5-trials/` is deleted.
