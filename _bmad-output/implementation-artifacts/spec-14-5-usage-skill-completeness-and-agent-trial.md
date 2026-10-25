---
title: 'Story 14.5: Usage Skill completeness and agent trial'
type: 'chore'
created: '2026-10-25'
status: 'done'
baseline_revision: '78b5c6eff1a22524db387d7df6e3873a653958ce'
review_loop_iteration: 0
followup_review_recommended: true
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

### 2026-10-25 — Review pass
- verdicts: 36 findings — high 0, medium 3, low 16, false 16, maybe-false 1
- findings:
  - `[false]` `[reject]` blind: `catalog_ids` is a second catalog parser; a bold id in an audit table breaks skill-check while rules-catalog stays green — `catalog::parse` also takes every table row whose first cell is a bold id (any section) and reports a duplicate as "duplicate catalog row", so both gates fail together; the spec mandates this parser plus the equality unit test.
  - `[false]` `[reject]` blind: missing `docs/` silently skips Q19 — without `docs/` the catalog `docs/discord-rules.md` cannot be read and Q17 reports `cannot read`, so the run fails loudly.
  - `[low]` `[reject]` blind: Q14 fix cell only needs a `.md` substring, not an existing reference — matches the spec's Q14 rule; resolving names against the reference set adds a check for a rare stale-name case.
  - `[low]` `[reject]` blind: `Lexer::strip` mislexes `'\"'` and raw strings — real but no skill or docs fence uses them (real repo passes); the fix adds raw-string/escape branches for an unlikely case.
  - `[low]` `[reject]` blind: index row `**CMD-01** name` gives a confusing "is not a catalog rule" — the spec's allowed id-cell forms are exact; rare, and fixing needs a new branch.
  - `[low]` `[reject]` blind: index section ends only at `# `/`## `; a second header row inside it is read as an id row — the real index is the file's last section; bare `##`/setext headings and split tables are unlikely; the fix adds parsing.
  - `[low]` `[reject]` blind: template `AGENTS.md`/`README.md` not covered by marker/UD-7 checks — tree is clean; widening scope is a new check for an unlikely regression.
  - `[low]` `[patch]` blind: patterns.md §11 / Common mistakes / adapters.md §3 call the id index "capped" but never cap it, and the read-then-write race is not explained — doc correction in the three places.
  - `[low]` `[patch]` blind: patterns.md snippets use undefined locale keys and helpers with no warning — one intro sentence that they are placeholders to add to the locales first.
  - `[medium]` `[patch]` blind: components-v2.md "`derive(Choices)` enums have no options helper; map `Severity::ALL` yourself" contradicts `defaults(options = SomeChoicesEnum)` in §10a of the same file and suggests unlocalized `label()` — replace the sentence with a pointer to `defaults(options = …)`.
  - `[low]` `[patch]` blind: CHANGELOG says the trial's gaps "are now covered" while the summary lists gaps no change addressed — reword CHANGELOG and add a "gaps not changed" list to the summary.
  - `[false]` `[reject]` blind: honest trial count is 3/5 (trial 1 fails clippy) and needs a rerun — D5 sets success to the implementer's `cargo build --release --locked`; clippy is reported, not gated; the forced rebuilds give 4/5.
  - `[false]` `[reject]` blind: summary commits absolute machine paths and a model/CLI string — the spec prescribes those paths and requires the model to be recorded; no personal data, no harm.
  - `[low]` `[patch]` blind: CLI tests lack Q16, Q12, Q3/Q4 and duplicate-id cases — only the Q16 wiring is unreached (Q3/Q4 run through `references()` hit by `q1_missing_reference`, Q12 through `index()` hit by `q9_index_missing_id`, Q17 has a CLI test); grouped with the vgap Q16 entry.
  - `[low]` `[reject]` blind: Q18 accepts relative link targets — the spec's Q18 row allows relative or absolute targets and the real skill uses the canonical URL; tightening is a new guard for an unlikely regression.
  - `[false]` `[reject]` blind: `v0-alpha`, `v0)` flagged; edge cases untested — exactly the spec's Q6 rule (only `.`+digit exempts); flagging them is the intended behaviour.
  - `[low]` `[reject]` edge: `md_lines` opens a fence on a prose line starting with inline triple backticks or a 4+-indented fence — every consequence is a loud problem (missing heading, no row), nothing in the skill triggers it; the fix adds guards.
  - `[low]` `[reject]` edge: `Lexer::strip` raw strings / escaped char literal — same root cause as the blind Lexer finding; rejected with it.
  - `[low]` `[reject]` edge: nested `/* /* */ */` comments — unlikely in skill examples; depth tracking adds state.
  - `[low]` `[reject]` edge: any row followed by a separator is skipped as a header — a catalog id in a header row is unlikely, and the result is a loud "has no row".
  - `[low]` `[reject]` edge: index section ends only at `# `/`## ` — same root cause as the blind section-end finding; rejected with it.
  - `[false]` `[reject]` edge: cheatsheet compound/range ids reported missing — the spec's Q16 defines the `[A-Za-z0-9_-]` token rule (`OPT-141` must not count), and the failure is loud.
  - `[false]` `[reject]` edge: Q18 rejects reference-style/autolinks and accepts `xdocs/discord-rules.mdx` — the spec's Q18 defines the `](…)` target containing `docs/discord-rules.md`; the real skill uses the canonical URL.
  - `[false]` `[reject]` edge: literal `135` in `real_catalog_ids_match_rules_catalog`/`real_repo_passes` — the spec requires asserting 135; a catalog change must update the index anyway, so skill-check fails first.
  - `[low]` `[reject]` edge: Q10 message uses the unwrapped id, not the raw cell — cosmetic; the matrix example message `ZZ-01 is not a catalog rule` matches.
  - `[false]` `[reject]` intent: agents had filesystem access to Noctis source, only no reads — the spec settles the path dependency into `crates/noctis` and the forbidden-read rule checked by transcript scan.
  - `[false]` `[reject]` intent: the trialed skill is not the shipped one — the spec allows skill fixes; the summary lists every post-round change (additive notes and rows), and the bar was met.
  - `[false]` `[reject]` intent: "compiles" should include clippy — D5 settles build-only success; clippy is reported.
  - `[false]` `[reject]` intent: shared `CARGO_TARGET_DIR` gave agents stale results — the spec's prompt prescribes the shared dir; the verdict rests on the implementer's forced rebuilds (D5), and the caveat is disclosed; stale success can only lower the agents' attempts, not raise the count.
  - `[maybe-false]` `[reject]` intent: component set comes from `BUILDERS`, not the FR-23/FR-29 lists — pre-existing check from an earlier story, no missing component named; settle by diffing FR-23/FR-29 against `BUILDERS`; would be low (every public builder is still forced into the skill by the public-API mention check).
  - `[false]` `[reject]` intent: AC2 checked by headings, not content — D1 settles "structure and coverage, not prose quality"; the content exists.
  - `[false]` `[reject]` intent: Q19 skips `UD7_SPAWN`, untagged/`rs` fences and inline code — the spec's Q19 row defines the scope (rust fences, the three identifier families).
  - `[false]` `[reject]` intent: Limits links point outside the template — the spec's Always clause mandates the canonical GitHub URL; rule meanings are local in the index and cheatsheet.
  - `[false]` `[reject]` intent: `Kv` key listing answered by a documented limitation — `Kv` has no list API in the snapshot; FR-50 asks the skill to document the API as it is, which it now does.
  - `[medium]` `[patch]` vgap: Q16 cheatsheet call in `completeness::check` is not reached by any CLI test — add `q16_cheatsheet_missing_id` (with the blind CLI-coverage finding).
  - `[medium]` `[patch]` vgap: Q19 over skill files is CLI-tested only through `docs/` — add `q19_listener_in_skill_example`.

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

## Auto Run Result

- **Summary:** `xtask skill-check` gains a completeness check (`xtask/src/skill_check/completeness.rs`): required references and headings, `[planned]`/`v0`/Epic-Story marker ban, rule-id troubleshooting index covering all 135 catalog ids, limits-cheatsheet coverage, component Limits links to `docs/discord-rules.md`, and a UD-7 scan of Rust examples in the skill and `docs/`. The Usage Skill was brought to pass it (new `concept-map.md`, `patterns.md` with Common mistakes, full troubleshooting index, every `[planned]` resolved against `xtask/public-api.txt`). The UJ-5 agent trial ran with 5 subagents on isolated template copies: 4/5 built (D5 criterion), recorded in `uj5-agent-trial-summary.md`.
- **Files changed:** `xtask/src/skill_check/{completeness.rs (new), mod.rs, components.rs, doctrine.rs}`, `xtask/src/main.rs`, `xtask/src/rules_catalog/mod.rs` (completeness gate and wiring); `xtask/tests/skill_check.rs` (CLI tests incl. Q16/Q19); `examples/project-template/.claude/skills/noctis/` SKILL.md and references (concept-map, patterns, troubleshooting index, cheatsheet, Limits links, marker cleanup); `examples/project-template/AGENTS.md`; `CHANGELOG.md`; `_bmad-output/implementation-artifacts/uj5-agent-trial-summary.md`; architecture memlog.
- **Review findings:** 36 total (high 0, medium 3, low 16, false 16, maybe-false 1). Patched 7 (medium 3, low 4): components-v2 Choices options contradiction replaced with `defaults(options = ...)` pointer; CLI tests `q16_cheatsheet_missing_id` and `q19_listener_in_skill_example` added; "capped" id-index wording corrected in patterns/Common mistakes/adapters; placeholder note for patterns snippets; CHANGELOG reworded and "gaps not changed" list added to the trial summary. Deferred 0. Rejected 29, each with its recorded reason in the Review Triage Log (lexer raw-string/escape edge cases, section-end parsing, Q14/Q18 strictness, trial methodology settled by D5 and spec prompt, literal 135 required by spec, etc.).
- **Follow-up review recommended: true** (first pass, 3 medium patched). Named risk: the two new CLI tests and the corrected skill wording were not re-reviewed; a follow-up should confirm the tests exercise the Q16/Q19 paths and the Choices guidance matches `defaults(options = ...)` in the public API.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, xtask deps-check, tw-grammar, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV). CI paused by maintainer.
- **Residual risks:** trial ran against a skill revised between rounds (changes listed in the summary); agents shared `CARGO_TARGET_DIR` (verdict rests on forced rebuilds); Kv/`Id` postcard runtime trap documented only, not guarded (D4); skill-check lexer ignores raw strings and nested comments.
