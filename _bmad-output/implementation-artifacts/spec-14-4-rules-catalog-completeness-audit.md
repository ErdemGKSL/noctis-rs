---
title: 'Story 14.4: Rules catalog completeness audit'
type: 'chore'
created: '2026-10-25'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** v1 needs proof that every `docs/discord-rules.md` rule is enforced and tested at its declared point (FR-75, FR-79, NFR-10, AD-3, AD-4). Today `xtask rules-catalog` passes with one stale pending entry (CID-04 `unit`, Story 7.1: tests exist but no name cites `cid_04`), nothing stops new non-P2 pending entries, and the catalog records neither why each runtime-only rule's input is dynamic nor the status of PRD Q1/Q4/Q5.

**Approach:** Rename the CID-04 App-build tests so they cite `cid_04` and empty `xtask/rules-pending.toml`; make the pending parser require `p2 = true` on every remaining entry; add a "v1 rules audit" section to the catalog (by-construction evidence, runtime-only rows with their dynamic-input reason, documentation-only/alias confirmations); pin that section to the classifier with an xtask unit test; record Q1/Q4/Q5 as unanswered with their conservative defaults.

## Boundaries & Constraints

**Always:**
- `cargo run --release -p xtask -- rules-catalog` prints exactly `rules-catalog: ok (135 rules: 130 covered, 0 pending, 3 documentation-only, 2 aliases)`.
- Rule ids, `Limits` defaults and `noctis_rules::RuleId` stay unchanged (no limit changes: Q1/Q4/Q5 are unanswered).
- Audit-section tables must not use the `| **ID** ...` bold-first-cell form (the catalog parser would read them as duplicate rule rows); first cells are bare ids (`MSG-04`).
- Existing problem messages of `rules-catalog` keep their exact text; new messages are exactly as specified below.
- Release-only cargo commands; no `cargo hack`.

**Never:**
- No public API change of any `noctis*` crate (tests rename only; xtask is internal), so no template, Usage Skill, `public-api.txt` or API hash edits.
- Do not make the audit section a `rules-catalog` CLI check (the CLI's generated test catalogs would need it); it is a unit test over the real catalog.
- Do not add the Kv/`Id` postcard guard (14.2 deferral) here; it is a public API change and belongs to Story 14.5.
- Do not change `components! {}` (empty literal) semantics: it stays valid (`crates/noctis/tests/components_macro.rs::empty_body`).

## I/O & Edge-Case Matrix

Pending file (`xtask/src/rules_catalog/pending.rs`, field `p2: bool`, serde default `false`, on both `[[rule]]` and `[[check]]`). The p2 check runs last, only on an entry that passed every existing validation; a flagged entry is still kept as pending (one problem line, no follow-on "missing evidence" line).

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| PD1 empty | file empty or comments only | no pending entries | none |
| PD2 tagged rule | `[[rule]] id="INT-01" evidence="unit" story="15.4" p2=true`, no evidence | pending, counted in summary, exit 0 | none |
| PD3 untagged rule | same without `p2` | problem `rules-pending.toml: INT-01 (unit): only a P2-feature rule may stay pending; add its test or tag the entry p2 = true`; entry kept pending | exit 1 |
| PD4 `p2 = false` | explicit false | same line as PD3 | exit 1 |
| PD5 wrong type | `p2 = "true"` or `p2 = 1` | whole file rejected with the toml message (existing P6 behaviour) | exit 1 |
| PD6 tagged but satisfied | PD2 plus a `#[test] fn int_01_x` | existing `INT-01: pending entry (unit, Story 15.4) already has a test; remove it from xtask/rules-pending.toml` only | exit 1 |
| PD7 invalid + untagged | unknown id / bad story / not required / duplicate, no `p2` | only the existing message for that defect (no p2 line) | exit 1 |
| PD8 untagged check | `[[check]] name="custom-id-constants" story="2.2"` | problem `rules-pending.toml: check custom-id-constants: only a P2-feature check may stay pending; tag the entry p2 = true`; check kept pending | exit 1 |
| PD9 tagged check | PD8 with `p2 = true` | check pending (constants comparison skipped) | none |
| PD10 non-ASCII id | `id = "ÇV2-01"` | existing `ÇV2-01 is not a catalog rule` | exit 1 |
| PD11 real repo | `xtask/rules-pending.toml` header only | summary line from Always | none |

Audit (`audit_problems(catalog_text: &str) -> Vec<String>` in `xtask/src/rules_catalog/audit.rs`, sorted, deduplicated). Sets derived from `catalog::parse`: **BC** = rows with a `Compile-time` tier classified by-construction; **RT** = `Required` rows whose every tier head (`split_tiers`) starts with `Runtime` or `Startup`; **DOC** = `DocOnly` rows; **ALIAS** = `Alias` rows. `audit_problems` ignores `catalog::parse`'s own problems (the real-catalog test asserts those separately). Listed ids = first cell, trimmed, of each `|` table row (each table's header row and its `|---` row skipped) under `### By construction`, `### Runtime only`, `### Documentation only and aliases` of `## v1 rules audit`.

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|
| A1 match | real catalog after this story | `[]` | none |
| A2 no section | no `## v1 rules audit` | `["v1 rules audit: section missing"]` | test fails |
| A3 missing subsection | section without `### Runtime only` | `v1 rules audit: subsection Runtime only missing` | test fails |
| A4 unlisted | an RT id absent from Runtime only | `v1 rules audit: <ID> is a runtime-only row but is not listed under Runtime only` (same form: `a by-construction row` / `a documentation-only or alias row`) | test fails |
| A5 wrong list | listed id not in that set (e.g. `CV2-03` under Runtime only) | `v1 rules audit: <ID> is listed under Runtime only but is not a runtime-only row` | test fails |
| A6 duplicate | id twice in one subsection | `v1 rules audit: <ID> is listed twice under <subsection>` | test fails |
| A7 unknown / malformed | first cell `ZZZ-01`, `**MSG-04**`, `` `MSG-04` `` or empty | `v1 rules audit: <cell> under <subsection> is not a catalog rule id` | test fails |
| A8 empty catalog | `""` | A2 only | test fails |

</intent-contract>

## Code Map

- `xtask/rules-pending.toml` -- only entry left: CID-04 `unit` (Story 7.1); its header documents the schema and "empty by Story 14.4, except entries for Epic 15 stories".
- `xtask/src/rules_catalog/pending.rs` -- `RuleEntry`/`CheckEntry` (`deny_unknown_fields`), `parse` (P4-P11 validation order: story, catalog row, required, duplicate; invalid entries dropped). Add `p2` here.
- `xtask/src/rules_catalog/mod.rs` -- module doc (classification/evidence/pending rules), `check()` summary, `#[cfg(test)] mod tests` (~l.630). Register `mod audit;`.
- `xtask/src/rules_catalog/catalog.rs` -- `Class`, `Row`, `Catalog`, `parse(markdown, &mut Problems)`, `split_tiers`, `HEADS`, `tier_kind` (by-construction = text before the first `;` of a Compile-time tier).
- `xtask/tests/rules_catalog.rs` -- CLI tests over generated repos; base fixture (`fn repo`, ~l.94) has an untagged `[[check]]`; pending tests `p1_p3_pending_and_missing` (~597), `p2_satisfied_pending`, `p4_p5_…`, `p6_…`, `p7_…`, `p8_duplicates`, `p9_…`, `p10_…`, `p11_…`, and one at ~l.508.
- `crates/noctis-core/src/registry/tests.rs` -- `mod routes` (~l.987): `duplicate_across_modules`, `duplicate_in_one_module`, `duplicate_across_families`, `three_handlers`, `distinct_routes`, `uses_satisfied`, `uses_unmounted`, `uses_deduplicated_and_ordered`; and `uses_of_a_pruned_route` (~l.1648). No other file references these names.
- `crates/noctis-core/src/responder/tests/components.rs` -- legacy-source component update assertion (~l.107-118); `modal_responder(..)`, `source(..)`; `Fixture::submit(declared, source)` in `responder/tests.rs:152`.
- `docs/discord-rules.md` -- rows CMD-07 (l.84), CMD-14 (94), LOC-04 (124), LOC-05 (125), REG-06 (150), CID-04 (178), CV2-02 (191), MOD-08 (241), MSG-02 (249), MSG-06 (253, empty Notes cell); §6 trybuild text l.635-637 on the pending file; `## Unconfirmed values` (l.641; rows CV2-04 = Q1, CV2-17 = Q4).
- `_bmad-output/planning-artifacts/prds/prd-noctis-rs-2026-10-19/prd.md` §10 -- Q1 (4000 text unit), Q4 (media url length), Q5 (legacy→V2 on modal-submit update); all unanswered.
- `CHANGELOG.md` -- `## [Unreleased]` with a "Performance budgets (PRD Q3)" subsection (Story 14.3).
- `.github/workflows-disabled/ci.yml` -- `rules-catalog` job already runs the gate; no change.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-core/src/registry/tests.rs` -- prefix the nine CID-04 tests listed in the Code Map with `cid_04_` (e.g. `cid_04_duplicate_across_modules`) -- gives the App-build tier its `unit` evidence.
- `xtask/rules-pending.toml` -- delete the CID-04 entry; rewrite the header: schema now includes `p2 = true` (required), the file is empty since Story 14.4 and may hold only P2-feature entries (prefix commands and other opt-in P2 features) -- AC 1.
- `xtask/src/rules_catalog/pending.rs` -- add `#[serde(default)] pub(crate) p2: bool` to both entry structs; after the duplicate check, report PD3/PD8 messages and keep the entry; extend its unit tests (PD3, PD4, PD8, PD9) -- enforces "only p2 rows remain".
- `xtask/src/rules_catalog/mod.rs` -- doc-comment the p2 rule and the audit test; `mod audit;`.
- `xtask/src/rules_catalog/audit.rs` (new) -- `audit_problems` per matrix A1-A8, unit tests for A2-A8 over small synthetic catalogs, and `real_catalog_audit_is_complete` reading `Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/discord-rules.md")` and asserting `audit_problems(..)` is empty and `catalog::parse` reported no problems -- pins AC 2 to the classifier.
- `xtask/tests/rules_catalog.rs` -- add `p2 = true` to the base fixture check and to every valid pending entry whose test is not about p2; add CLI tests `p12_untagged_rule_and_check` (PD3+PD8 exact lines) and `p13_p2_wrong_type` (PD5) -- keeps existing expectations exact.
- `crates/noctis-core/src/responder/tests/components.rs` -- add `#[tokio::test] async fn cv2_02_modal_submit_update_of_a_legacy_source_clears_it`: `fx.submit(Declared::Update, source(MessageFormat::Legacy, Ephemerality::Public))`, `modal_responder(&shared).update(v2("a"))` → the type-7 body equals the component legacy case (flags 32768, `content: null`, `embeds: []`, `poll: null`, `sticker_ids: []`) -- evidence for the Q5 interpretation.
- `docs/discord-rules.md` -- (a) REG-06 Enforcement `(out of scope for v1)`; (b) MSG-06 Notes: no typed message carries `nonce` or stickers in v1 (V2 forbids stickers, CV2-02; `LegacyMessage` is plain text, FR-23); `noctis_rules::message::validate_nonce`/`validate_sticker_ids` serve raw-REST callers (unit test `msg_06_nonce_and_stickers`); (c) §6 l.637: "it has been empty since Story 14.4; an entry must carry `p2 = true` and belong to a P2 feature"; (d) Unconfirmed values: prefix CV2-04's and CV2-17's interpretation with `PRD Q1 (unanswered as of 2026-10-25):` / `PRD Q4 (…):`, add row `CV2-02 (PRD Q5)` | whether an update answering a modal submit may switch a legacy message to V2 | Noctis converts as for a component update (flag set; `content: null, embeds: [], poll: null, sticker_ids: []`); a Discord rejection surfaces as the typed HTTP error, no fallback; (e) new `## v1 rules audit` section after `## Compile-time strategy` (before `## Unconfirmed values`) per Design Notes -- AC 2-4.
- `CHANGELOG.md` -- under `[Unreleased]` add `### Rules catalog (v1 audit)`: pending list empty and p2-only; CID-04 tests cite their id; audit section and its test; Q1/Q4/Q5 unanswered, defaults kept; MSG-06 and REG-06 wording.

**Acceptance Criteria:**
- Given the repository after this story, when `cargo run --release -p xtask -- rules-catalog` runs, then it exits 0 with `rules-catalog: ok (135 rules: 130 covered, 0 pending, 3 documentation-only, 2 aliases)` and `xtask/rules-pending.toml` has no `[[rule]]` or `[[check]]` table.
- Given the catalog, when `real_catalog_audit_is_complete` runs, then the `### By construction` list is exactly OPT-12, OPT-14, LOC-04, CTX-02, CTX-03, META-05, INT-03, INT-04, INT-06, INT-08, INT-09, INT-10, CID-05, CID-07, CV2-01, CV2-02, CV2-12, CV2-14, MSG-03, MSG-05, MSG-07, ATT-05, ATT-07, WH-01, WH-03, GW-07 (Gate 2's 25 plus LOC-04), each row naming its `// [<ID>]` fixture or doctest file.
- Given every rule whose tiers are all Runtime/Startup (LOC-06, META-04, REG-05, INT-01, INT-02, INT-07, INT-11, INT-12, INT-13, CV2-16, MOD-09, MSG-04, MSG-06, MSG-08, MSG-09, ATT-01, ATT-02, ATT-03, ATT-04, WH-05, RL-01, RL-02, RL-03, RL-04, GW-04, VC-01, VC-02), when the audit is read, then each has a `### Runtime only` row stating why its input is dynamic, and none has static input that is checked only at runtime.
- Given the documentation-only rows, when the audit is read, then CMD-14 (permitted), REG-06 (out of scope for v1), MSG-02 (no embed API in v1) and the aliases LOC-05→CMD-05, MOD-08→CID-01 are table rows, and prose below the table confirms CMD-07's discovery restriction is documentation-only and LOC-04's "partial localization permitted" holds (a missing non-default key is a warning, never an error).
- Given PRD Q1, Q4 and Q5 are unanswered, when the Unconfirmed values table is read, then each is named with its conservative interpretation and every `Limits` default is unchanged.
- Given an untagged pending entry, when `rules-catalog` runs, then it fails with the PD3/PD8 line.

## Spec Change Log

## Review Triage Log

## Design Notes

**Audit section layout** (bare ids in first cells):

```markdown
## v1 rules audit

Story 14.4 (2026-10-25) audited every row before v1. `real_catalog_audit_is_complete` (xtask) keeps these lists equal to what `rules-catalog` derives from the Enforcement column.

### By construction
| Rule | Evidence (`// [<ID>]`) |
|---|---|
| CV2-02 | `crates/noctis-ui/src/message/mod.rs` doctest |

### Runtime only
| Rule | Why its input is dynamic |
|---|---|
| MSG-09 | `Components` stays extendable after `components!` (`.push`), and `components! {}` is a valid start, so only the tree at send time is final. |

### Documentation only and aliases
| Rule | Status |
|---|---|
| LOC-05 | alias of CMD-05 (longest localization counts in the 8000 budget) |
```

Find each by-construction evidence file with `grep -rn "// \[<ID>\]" crates`. Dynamic-input reasons (implementer words them): LOC-06 names/locales arrive in the payload; META-04 app settings read at Publish; REG-05 remote command set and Discord-side daily counter; INT-01/INT-02 wall-clock from gateway receipt; INT-07 handler-computed per keystroke; INT-11 install owners per interaction plus a runtime counter; INT-12 call order; INT-13 per-request routing; CV2-16 raw JSON; MOD-09 user-submitted values; MSG-04 snowflake lists of unknown length; MSG-06 raw-REST values only; MSG-08 target message attachments known from Discord; MSG-09 as above; ATT-01..03 file bytes, interaction `attachment_size_limit`, kept files; ATT-04 numbering of the runtime file set; WH-05 channel kind known only from Discord or stored credentials; RL-01/03/04 server responses; RL-02 cluster config from the environment at startup; GW-04 send rate over time; VC-01 runtime shard count/range; VC-02 per-guild call state.

**Decisions:** D1 pending entries need `p2 = true` (makes "only p2 rows remain" enforceable; all Epics 15-17 are done so nothing remains). D2 the audit is a unit test over the real catalog, not a CLI check (avoids rewriting every generated CLI catalog). D3 LOC-04 stays an enforced by-construction rule (FR-42 strengthened it after Gate 2); its "permitted" half is confirmed in prose, so the by-construction list is Gate 2's 25 + LOC-04 (INT-05 stays diagnosed by the first-`;` rule). D4 Q1/Q4/Q5 cannot be answered offline: defaults stay; Q5's interpretation follows maintainer decision §2 and is pinned by the new CV2-02 modal test. D5 the Kv `Id` guard deferred by 14.2 moves to Story 14.5 (public API).

## Verification

**Commands:**
- `cargo fmt --all` -- no diff afterwards
- `cargo test --release -p xtask` -- all pass, including `audit::` and new `p12`/`p13` tests
- `cargo test --release -p noctis-core registry` and `cargo test --release -p noctis-core cv2_02` -- pass
- `cargo run --release -p xtask -- rules-catalog` -- exact AC 1 line
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `tools/story-loop/bg.sh cargo test --release --workspace` -- pass
- `cargo run --release -p xtask -- api-snapshot` and `cargo run --release -p xtask -- skill-check` -- pass unchanged (no API change)
- `cargo run --release -p xtask -- deps-check`, `cargo deny check`, `cargo +1.91 check --release --workspace` -- pass
