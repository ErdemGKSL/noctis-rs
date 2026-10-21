---
title: 'Story 8.2: Deterministic renders, select freshness and safe text'
type: 'feature'
created: '2026-10-21'
status: 'done'
baseline_revision: '9bb875b6936c4f53935e9e50ce818e4f2fb9f3ac'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** FR-25 promises that a render function is safe to call after every click. Three parts of that promise are missing:
- Select freshness is only probabilistic. A send's `N` nonce is drawn at random and can equal the nonce the client already holds (1 in 3844). The client then keeps the user's old local selection.
- Runtime CID-02 skips unresolved ids (select, signed and by-Ref Routes), and its error has no help line.
- `escape_markdown` and `escape_mentions` do not exist. The template and the Usage Skill already call them, marked `[planned, Story 8.2]`.

**Approach:**
- Make the nonce fresh by construction. Each send avoids every nonce found in the interaction's raw JSON and every nonce this run already drew.
- Add the two escape helpers to `noctis-ui` and re-export them from `noctis::ui`.
- Make runtime CID-02 compare unresolved Route ids by Route + Payload identity, and add a help line suggesting a distinguishing Payload field, at both runtime and compile time.
- Prove determinism with an end-to-end snapshot test.
- Reconcile the docs, the template, the skill and the API snapshot.

## Boundaries & Constraints

**Always:**
- Every send goes through `Shared::prepare` → `resolve_ids`, which draws one nonce per send for the whole message. The draw now avoids `Shared`'s avoid-set. The avoid-set starts with the nonces scanned from the raw interaction, and each drawn nonce is added to it.
- The nonce scan allocates nothing. Use a fixed-capacity `NonceSet` (8 slots, a ring that overwrites its oldest entry when full; no new dependency). It must stay within the `alloc_dispatch` and `memory_flat` budgets.
- The escape helpers are pure, total functions: `fn(&str) -> String`, `#[must_use]`, never panic. They only insert characters and never delete any.
- CID-02 never truncates in Lenient mode (unchanged). Modal input-key CID-02 keeps its current message and gets no help line.
- Release-only builds. trybuild stays `compile_fail` only. No `unwrap`/`expect` outside tests.

**Never:**
- No automatic escaping anywhere: the author calls the helpers.
- No change to the `custom_id` layout, nonce width, Ref minting or the codec.
- No new rule ids or Limits. No new external crates. `insta` is already a workspace dependency; adding it to `noctis-testkit`'s dev-deps is allowed.
- No testkit snapshot API (Story 8.4) and no Budget (Story 8.3).
- By-Ref Routes keep minting a fresh Ref per send. They are outside the "identical except nonces" guarantee, and this is documented.

## I/O & Edge-Case Matrix

`Z` = U+200B. Rust string notation.

**`escape_markdown`.** Inline set, escaped everywhere: `\ * _ ~` `` ` `` `| [ ]`. Line-start set, escaped only at the start of the text or after `\n`, after optional ASCII spaces and tabs: `# > - +`, plus the `.` that ends a run of ASCII digits.

| # | Input | Output |
|---|---|---|
| M1 | `""` | `""` |
| M2 | `"plain text 123"` | unchanged |
| M3 | `"**b** __u__ ~~s~~"` | `"\\*\\*b\\*\\* \\_\\_u\\_\\_ \\~\\~s\\~\\~"` |
| M4 | `"\|\|sp\|\|"` | `"\\\|\\\|sp\\\|\\\|"` |
| M5 | `` "`c`" `` and a ```` ``` ```` fence | every backtick gets `\` |
| M6 | `"[x](https://e.com)"` | `"\\[x\\](https://e.com)"` (masked link dead; the URL is untouched) |
| M7 | `"a\\b"` | `"a\\\\b"` (the user's backslash stays visible) |
| M8 | `"# h"`, `"## h"`, `"-# sub"` | `"\\# h"`, `"\\## h"`, `"\\-# sub"` (only the first char) |
| M9 | `"> q"`, `">>> q"`, `"- i"`, `"+ i"` | `"\\> q"`, `"\\>>> q"`, `"\\- i"`, `"\\+ i"` |
| M10 | `"1. i"`, `"  12. i"`, `"1.5 kg"` | `"1\\. i"`, `"  12\\. i"`, `"1\\.5 kg"` |
| M11 | `"a # b - c > d 3. e"` | unchanged (not at a line start) |
| M12 | `"x\n# y"`, `"x\r\n- y"`, `"\n\n> z"` | `"x\n\\# y"`, `"x\r\n\\- y"`, `"\n\n\\> z"` |
| M13 | `"\t# t"` | `"\t\\# t"` |
| M14 | `"😀*é"`, `"＊ｆ"` (fullwidth) | `"😀\\*é"`, unchanged |
| M15 | `"<@1> @everyone"` | unchanged (mentions are not Markdown) |
| M16 | 4000 × `"*"` | 8000 chars. The Limits validator counts the escaped text (Strict → CV2-04 error; Lenient truncates it) |

**`escape_mentions`.** Insert `Z` after `@` when it is followed by `everyone` or `here` (ASCII case-insensitive), and after `<` when it is followed by `@`, `#` or `/`.

| # | Input | Output |
|---|---|---|
| E1 | `""` | `""` |
| E2 | `"@everyone"`, `"@HERE"` | `"@Zeveryone"`, `"@ZHERE"` |
| E3 | `"<@1>"`, `"<@!1>"`, `"<@&2>"` | `"<Z@1>"`, `"<Z@!1>"`, `"<Z@&2>"` |
| E4 | `"<#3>"`, `"</ban:4>"` | `"<Z#3>"`, `"<Z/ban:4>"` |
| E5 | `"@someone"`, `"a@b.com"`, `"<:e:1>"`, `"<t:1:R>"`, `"<https://x>"` | unchanged |
| E6 | `"mail@here.io"` | `"mail@Zhere.io"` (over-escaping is harmless) |
| E7 | `"@@everyone"`, `"<<@1>"` | `"@@Zeveryone"`, `"<<Z@1>"` |
| E8 | `"@"`, `"<"`, `"@ever"`, `"<@"` (truncated) | `"@"`, `"<"`, `"@ever"`, `"<Z@"` |
| E9 | `"😀<@1>"` | `"😀<Z@1>"` |
| E10 | any `x` | `escape_markdown(&escape_mentions(x)) == escape_mentions(&escape_markdown(x))` (property) |
| E11 | any `x` without `Z` | removing every `Z` from `escape_mentions(x)` gives `x`, and the output contains none of `<@ <# </ @everyone @here` in any case (property) |
| E12 | any `x` | `unescape(escape_markdown(x)) == x`, where `unescape` drops a `\` before ASCII punctuation (property: what Discord renders is the user's text) |

**Nonce scan `nonces_in(raw: &[u8]) -> NonceSet`.** For each `"custom_id"` key: skip whitespace, `:` and whitespace, read the `"…"` string, and `noctis_codec::decode` it. Collect its `nonce()` when present.

| # | Raw | Result |
|---|---|---|
| N1 | no `custom_id` / not JSON / empty | empty set |
| N2 | `{"data":{"custom_id":"<select id, nonce ab>"}}` | `{ab}` |
| N3 | `"custom_id" : "<id>"` (spaced) | found |
| N4 | the button click's data (no `N`) + a message select (`cd`) | `{cd}` |
| N5 | an id that is not Noctis / malformed / an inline Route without `N` | ignored |
| N6 | an unterminated string, or `"custom_id"` as the last bytes | ignored, no panic |
| N7 | `\"custom_id\"` inside a content string | not matched (escaped quotes) |
| N8 | the same nonce twice | stored once |
| N9 | 10 distinct nonces | the last 8 kept (ring) |
| N10 | a modal submit: input keys `custom_id` + a source message select `ef` | `{ef}` |

**Draw `draw_nonce(avoid: &NonceSet, fill)`.**

| # | Case | Result |
|---|---|---|
| D1 | fill gives a value ≥ the rejection limit, then a valid one | the valid one |
| D2 | fill gives an avoided nonce first, then `x` | `x` |
| D3 | fill error | `ResponderError::Encode { reason: "no select nonce could be drawn: …" }` |
| D4 | avoid empty | the first valid draw, uniform over `0..=3843` (as shipped) |

**Responder and CID-02.**

| # | Scenario | Expected |
|---|---|---|
| R1 | select click carrying nonce `ab`, handler `ctx.update(render())` | one type-7 callback. Its select `custom_id` has `N ≠ ab`, and the render's declared defaults (`default: true` option, `default_values`) are in the JSON |
| R2 | button click; the source message holds a select with `cd` | the new nonce is `≠ cd` |
| R3 | one run: `ctx.update(render())`, then `ctx.respond(render())` | two sends, two different nonces |
| R4 | after the auto-defer (type 6), `ctx.update` → `PATCH @original` | the nonce is still `≠` the source nonce |
| R5 | a message without selects | no nonce drawn (unchanged) |
| R6 | the same command invoked twice, render `render_panel(&state)` | both callback bodies are equal after masking each select id's 2 `N` chars. Snapshot `select_freshness__panel_render.snap` |
| C1 | two `Page { page: 1 }` buttons (resolved) | CID-02 at the second path, note `first used at <first path>`, help `HELP` (below) |
| C2 | two identical select Routes, unresolved, via `noctis::ui::validate` | one CID-02 at the second path. ``Actual::Text("route `<id>`: equal Payload")`` (`<id>` = the Route id), note, help (replaces the test `cid_02_unresolved_ids_are_skipped`) |
| C3 | the same select Route with different Payloads, unresolved | passes |
| C4 | two identical by-Ref buttons, unresolved | CID-02 (slots compare equal) |
| C5 | two identical signed Routes, no App key | CID-02 (placeholder signer) |
| C6 | an encode error while building the key | not compared. The Responder later reports `Encode` |
| C7 | the same duplicate after `resolve_ids` (Responder) | CID-02 with the resolved id as `Actual`, no request sent, `Invalid` |
| C8 | `components!` with `Press { n: 1 }` twice | compile error with the new help (re-blessed `CID-02_components_duplicate_route.stderr`) |
| C9 | a modal with a duplicate input key | unchanged message, no help |
| S1 | V2 message listing `<@1> <@&2> @everyone`, default mentions | `allowed_mentions == {"parse":[]}` |
| S2 | the same with `.mentions(Mentions::none().users(Users::only([Id::new(1)])))` | `{"parse":[],"users":["1"]}` (the existing serialization) |

`HELP` = ``give the components a Payload field that tells them apart, for example the target page of a prev/next pair (`Page { page: 2 }` and `Page { page: 4 }`)``.

</intent-contract>

## Code Map

- `crates/noctis-ui/src/limits/mod.rs`:
  - L441-464: `check_custom_ids`. Change the key from `id.get()` to the new duplicate key, and add `.with_help(HELP)`.
  - L384-418: `check_modal_keys`. Unchanged.
  - Module docs: L1-12 and L165-185 (the "Unresolved `custom_id`s are not compared" sentence must go).
- `crates/noctis-ui/src/limits/walk.rs` L32: `Site::custom_id` returns `&CustomId`. Reuse it.
- `crates/noctis-ui/src/route.rs`:
  - L253-290: `ErasedRoute`/`Erased::encode`.
  - L306-418: `CustomId`. Add `pub(crate) fn duplicate_key(&self) -> Option<DupKey>`:
    - `Resolved(ArrayString<MAX_ID_LEN>)` when `get()` is `Some`;
    - otherwise encode with the Parts `resolve_for_send` builds (nonce `SelectNonce::new(0)` only if `has_select()`, a private `struct PlaceholderSign` returning `Tag::from_u64(0)` only if `is_signed()`), giving `Inline(ArrayString)` or `Slot(PendingRef)`;
    - `Err` → `None`.

    `DupKey` derives `Hash, Eq`. `PendingRef` is already `Hash + Eq` (used as a `HashMap` key in `responder.rs` L1488).
- `crates/noctis-ui/src/lib.rs`:
  - L209-253: the module list and re-exports. Add `mod escape;` and `pub use escape::{escape_markdown, escape_mentions};`.
  - L188: the crate docs.
- `crates/noctis/src/lib.rs`:
  - L1905+: the `pub mod ui` docs (add a "User text" bullet).
  - L2443-2458: the re-export list. Add both helpers.
- `crates/noctis-macros/src/components/lower.rs` L388-418: `cid_02`. Replace the help string at L409 with `HELP`. The unit test at L802-823 checks the message only.
- `crates/noctis/tests/ui/cid/CID-02_components_duplicate_route.stderr`: re-bless (the help line).
- `crates/noctis-core/src/responder.rs`:
  - L1133-1186: `SourceMessage` and `from_interaction`. Add `pub(crate) nonces: NonceSet`, filled by `nonces_in(raw)` over the whole raw interaction. Update the 7 `SourceMessage {` literals (`grep -rn "SourceMessage {" crates/noctis-core/src`, incl. `DETACHED_SOURCE` and the `source_message_from_raw_json` test at `responder/tests/components.rs` L693).
  - L1236-1262: `Shared`. Add `nonces: Mutex<NonceSet>`, initialised in `build` from `source.map(|s| s.nonces)`.
  - L1453-1500: `resolve_ids`. Draw through `nonce::draw_nonce(&avoid, getrandom fill)` and insert the drawn nonce under the lock (never held across `.await`).
  - L2134-2153: the old `draw_nonce`. Move it into `nonce.rs`.
- `crates/noctis-core/src/responder/nonce.rs` (new; `mod nonce;` in `responder.rs`): `NonceSet`, `nonces_in` and `draw_nonce`, plus `#[cfg(test)] mod tests` covering N1-N10 and D1-D4.
- `crates/noctis-core/src/pipeline/component.rs` L586-600: `shared_for` already passes `SourceMessage::from_interaction(raw.as_bytes())`. No change beyond the field.
- Test patterns:
  - `crates/noctis-core/src/responder/tests.rs`: `Fixture` (L44-180).
  - `responder/tests/components.rs`: `source()` L26.
  - `crates/noctis-testkit/tests/responder_ergonomics.rs`: `Running`, `click` L355, `source_message` L344, `body`, `assert_interaction_request`.
  - `crates/noctis-ui/tests/validate.rs` L740-826 (the CID-02 tests, `resolve` helper L252, `pick()`, `user_select`, `string_select`).
- Template, under `examples/project-template/`:
  - `src/modules/profile/view.rs` L30 and L41 (the bio becomes `|b| escape_mentions(&escape_markdown(b))`, import `escape_mentions`);
  - `src/modules/dashboard/view.rs` L43 and L126;
  - `src/modules/profile/handlers.rs` L84;
  - `AGENTS.md` (near L53).
- Skill, under `examples/project-template/.claude/skills/noctis/`:
  - `references/components-v2.md`: L139-146, L181, L377 and the CID-02 table row L884;
  - `references/views-and-recipes.md` §5 L551-559 (also drop the stale "declare `select` on every select Route");
  - `references/troubleshooting.md` (a CID-02 entry);
  - `references/limits-cheatsheet.md` L283;
  - `SKILL.md` L23 (the API hash).
- `docs/discord-rules.md`: the CID-02 row (L175), the MSG-05 row (L251), and the layout block (L313-325, a freshness sentence after it).
- Spine `ARCHITECTURE-SPINE.md` AD-5 (L213-218): add an `Amended (2026-10-21)` bullet. The memlog already has the three Story 8.2 `(decision)` lines.

## Tasks & Acceptance

**Execution (in order):**

1. `crates/noctis-ui/src/escape.rs` (new):
   - Add `escape_markdown` and `escape_mentions`, exactly per M1-M16 and E1-E9.
   - Rustdoc on each: what is escaped, why U+200B (order-independent), "escape once" (the helpers are not idempotent), and a runnable example.
   - In-file unit tests, one per matrix row M1-M15 and E1-E9 (named `escape_markdown_<case>`, `escape_mentions_<case>`).
   - Wire it into `lib.rs` (the re-exports and the crate docs).
2. `crates/noctis-ui/src/route.rs` + `limits/mod.rs`:
   - add `DupKey`, `PlaceholderSign` and `CustomId::duplicate_key`;
   - make `check_custom_ids` use them: `Actual::Text(id)` when resolved, else ``format!("route `{route}`: equal Payload")``, the note as now, and `.with_help(HELP)` (a private `const HELP: &str`; `noctis-macros` repeats the identical literal, and both tests pin it);
   - update the module and `validate` docs.
3. `crates/noctis-ui/tests/validate.rs`:
   - add `.with_help(HELP)` to the expected values of the existing CID-02 tests;
   - replace `cid_02_unresolved_ids_are_skipped` with `cid_02_unresolved_duplicates_are_reported` (C2), and add `cid_02_unresolved_distinct_payloads_pass` (C3), `cid_02_by_ref_duplicates` (C4), `cid_02_signed_duplicates_without_a_key` (C5) and `cid_02_help_suggests_a_distinguishing_field` (C1);
   - keep `cid_02_cost_with_unresolved_ids`.
4. `crates/noctis-macros/src/components/lower.rs`: set the help to `HELP`, then re-bless `crates/noctis/tests/ui/cid/CID-02_components_duplicate_route.stderr` with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`. Check that only the help line changed.
5. `crates/noctis-core/src/responder/nonce.rs` (new) + `responder.rs`: `NonceSet`, `nonces_in` and `draw_nonce` (injectable `fill: impl FnMut(&mut [u8; 2]) -> Result<(), getrandom::Error>`), wired as in the Code Map. Unit tests N1-N10 and D1-D4.
6. `crates/noctis-core/src/responder/tests/components.rs`:
   - `fr_25_update_avoids_the_source_nonce` (R1/R4, a source with nonce `ab`, 64 iterations);
   - `fr_25_each_send_of_a_run_gets_a_new_nonce` (R3);
   - `fr_25_no_select_no_nonce` (R5);
   - `cid_02_duplicate_after_resolution_sends_nothing` (C7);
   - update `source_message_from_raw_json` for the `nonces` field.
7. `crates/noctis-testkit/Cargo.toml`: dev-dep `insta = { workspace = true, features = ["json"] }`. Then `crates/noctis-testkit/tests/select_freshness.rs` (new; copy the helpers from `responder_ergonomics.rs`):
   - a `#[derive(StringSelectMenu)]` `Pick` with `defaults(placeholder = "Pick", options = Mode)`, a `#[derive(UserSelectMenu)]` `Who`, and `Page { page: u16 }` buttons;
   - `render_panel(state)`, which uses `escape_markdown(&state.name)`, lists `<@1>`, marks the current option `.default(true)` and sets user `default_values`;
   - tests `select_click_update_gets_a_fresh_nonce` (R1), `button_click_avoids_the_message_nonce` (R2) and `same_state_renders_identical_json` (R6: mask the nonces, `assert_eq!`, `insta::assert_json_snapshot!("panel_render", masked)`; also assert S1 on the body).

   Hand-write the incoming select `custom_id`s with a known nonce, or build them with `noctis::codec` encoding if exported. Comment the layout either way. Create the snapshot with `INSTA_UPDATE=always`, review it, then re-run without the variable.
8. `crates/noctis/tests/escape.rs` (new, `proptest`):
   - properties E10, E11 and E12 (256 cases each, ASCII-heavy and Unicode strategies);
   - `msg_05_listed_mentions_ping_nobody` (S1/S2) over `V2Message::create_body`.
9. `crates/noctis/src/lib.rs`: the re-exports and the `ui` module docs. Run `cargo run --release -p xtask -- api-snapshot --bless`, then put the printed hash on `SKILL.md`'s `API hash` line.
10. Template:
    - remove both `[planned, Story 8.2]` markers;
    - in profile `view.rs`, import and use `escape_mentions` on the bio;
    - `AGENTS.md`: add the bullet "Pass user text (names, reasons, bios) through `escape_markdown` (and `escape_mentions` where mentions must not render) before putting it in a Text Display; every select gets a fresh nonce per send, so always set its defaults to the current value."
11. Skill: drop every `[planned]` on the helpers, and document:
    - the two helpers (escape sets, U+200B, escape once, order-free);
    - guaranteed freshness (avoids the clicked/source nonce and the run's previous sends; by-Ref ids differ per send);
    - the CID-02 help and the comparison of unresolved ids (`components-v2.md` L884);
    - a troubleshooting row for `[CID-02] … help: give the components a Payload field …`.
12. `docs/discord-rules.md`:
    - CID-02: unresolved ids are compared by Route + Payload; the help; the new test names;
    - MSG-05: the escape helpers and `msg_05_listed_mentions_ping_nobody`;
    - the layout note: "`N` is drawn per send and never equals a nonce of the interaction being answered or an earlier send of the same run (FR-25)".

    Spine AD-5: an `Amended (2026-10-21)` bullet saying the same.
13. Run every Verification gate.

**Acceptance Criteria:**
- Given `render_panel(&state)` called for two invocations with the same state, when both callback bodies are serialized, then they are equal once each select id's two `N` characters are masked, and they match the committed insta snapshot.
- Given a select or button interaction on a message whose select ids carry nonce `X`, when the handler sends `ctx.update(render(..))` (type 7 or `PATCH @original`), then every select id in the body carries a nonce `≠ X`, the render's declared defaults are in the body, and a second send in the same run carries yet another nonce.
- Given user text, when it passes through `noctis::ui::escape_markdown` / `escape_mentions`, then the outputs match rows M1-M16 and E1-E9 and the E10-E12 properties hold; and a message listing `<@id>` sends `allowed_mentions {"parse":[]}` unless `.mentions(..)` opts in.
- Given two Route components with the same Route and Payload in one message, resolved or not, when it is validated (`noctis::ui::validate` or a Responder send), then CID-02 names the later path, notes the first, and its help suggests a distinguishing Payload field; `components!` shows the same help at compile time.
- Given the new public API, when `skill-check` runs, then it passes: both helpers are mentioned, no `Story 8.2` `[planned]` marker remains in the template or skill, and the `API hash` matches.

## Spec Change Log

## Review Triage Log

### 2026-10-21 — Review pass
- verdicts: 26 findings — high 0, medium 6, low 10, false 9, maybe-false 1
- findings:
  - `[medium]` `[patch]` (blind) 8-slot ring shared by interaction nonces and draws evicts the source nonce after ~8 sends, breaking the "never an earlier/interaction nonce" guarantee — verified `NonceSet::insert` overwrites the oldest slot and `Shared::draw_nonce` inserts every draw into the same ring; patch: keep scanned nonces in a separate never-written set, avoid both, test >8 draws, and word the docs as "the interaction's nonces and the run's last eight sends".
  - `[low]` `[reject]` (blind) a resolved select id plus an unresolved one of equal Route/Payload passes pre-send CID-02 but may collide after resolution — real (`DupKey::Resolved` never equals `Inline`), but reaching it needs an author to hand-resolve via the hidden `resolve` or reuse a sent id; fixing it adds a re-keying branch.
  - `[medium]` `[patch]` (blind) FR-25 tests through `Shared` are probabilistic and `Shared` hard-wires `fill_os` — grouped with the verification-gap finding; patch: deterministic avoid-set assertions plus a `#[cfg(test)]` scripted fill.
  - `[low]` `[reject]` (blind) CID-02 help literal is duplicated across crates with no cross-check — the spec deliberately chose a repeated literal pinned by each crate's test; drift is unlikely and a shared constant adds cross-crate public surface.
  - `[low]` `[reject]` (blind) `escape_mentions` misses `<id:customize>`-style guild navigation links — real but they ping nobody and are rare in user text; the fix adds a new arm beyond matrix E1-E9 and the E11 property.
  - `[low]` `[patch]` (blind) `escape_markdown`/module docs claim text "renders as written" though timestamps, custom emoji and links stay live — verified the wording in `escape.rs` L5-6/L21-22; patch: reword the docs to list what stays live (docs only).
  - `[maybe-false]` `[reject]` (blind) lone `\r`, U+2028/2029 and non-ASCII leading whitespace are not treated as line starts — would need Discord's renderer behavior for these inputs to settle; even if true only `low` (stray heading/list formatting from unusual whitespace).
  - `[false]` `[reject]` (blind) template names skip `escape_mentions` against its own guidance — the guidance (AGENTS.md L41, views-and-recipes §5, troubleshooting) says `escape_mentions` only "where mentions must not render", the Code Map specifies it for the bio only, and `allowed_mentions` defaults to `parse: []`.
  - `[false]` `[reject]` (blind) `duplicate_key` silently skips ids whose encode fails; re-encoding cost — matrix C6 deliberately settles "not compared, the Responder later reports Encode"; no measured cost and the dhat budgets are a gate.
  - `[false]` `[reject]` (blind) CID-02/MSG-05 text changed in docs but not in `noctis-rules` — `noctis-rules` carries only the rule ids (`rule.rs` L57), no help or enforcement text, and no Limit or rule changed.
  - `[low]` `[patch]` (blind) doc lines edited in `limits/mod.rs` were not re-wrapped — verified L8 (117 cols) and L175 (120 cols) come from this diff; patch: re-wrap those two paragraphs.
  - `[low]` `[reject]` (blind) `std::sync::Mutex` held across the `getrandom` syscall — never across `.await` (as the Code Map requires); concurrent sends of one run are rare and a copy-draw-retry adds a loop for a microsecond wait.
  - `[low]` `[reject]` (blind) testkit freshness tests rely on fixed sleeps and start two Apps — the helpers were copied from `responder_ergonomics.rs` as the spec instructed; replacing them with notifications or a paused clock is new harness machinery.
  - `[false]` `[reject]` (blind) `same_state_renders_identical_json` would pass if the nonce were dropped or constant — those regressions are caught elsewhere: a dropped nonce fails `nonce_of` decoding in R1/R2 and a constant one fails `fr_25_each_send_of_a_run_gets_a_new_nonce` (4 distinct values).
  - `[medium]` `[patch]` (edge) a run with >8 sends evicts the interaction nonces from the shared ring — same root cause as the first blind finding; same patch.
  - `[low]` `[patch]` (edge) `resolve_ids` draws a nonce for messages with only unresolved signed/by-Ref ids — verified the draw happens for the first unresolved id regardless of shape, contradicting R5 and filling the ring; patch: draw only for `has_select()` ids and extend `fr_25_no_select_no_nonce`.
  - `[low]` `[reject]` (edge) an already-resolved select id carrying the source nonce is kept — needs a hand-resolved or reused id (select ids from `.build()` are always unresolved); the fix adds a decode-and-reject guard.
  - `[false]` `[reject]` (edge) a missing/unparseable `message` drops the data nonce — component interactions always carry `message` with id and channel; a slash-opened modal submit has no message and its data ids are input keys, never select nonces.
  - `[low]` `[reject]` (edge) mixed resolved + unresolved equal select ids pass pre-send CID-02 — same root cause and verdict as the second blind finding.
  - `[medium]` `[patch]` (edge, claim) "never equals an earlier send of the same run" is false past 8 draws — same root cause as the first blind finding; the patch rewords the claim to the bounded guarantee.
  - `[medium]` `[patch]` (vgap, pre-verified) FR-25 avoid-set wiring in `Shared` is only checked by random draws — filed disposition `patch`; grouped with the third blind finding; patch adds the deterministic tests.
  - `[medium]` `[patch]` (vgap, other) ring of 8 evicts the interaction's nonces after ~7 select-bearing sends — same root cause as the first blind finding; same patch.
  - `[false]` `[reject]` (intent 1) AC2 implemented as a guarantee (R2b) rather than a fresh random draw; client surface not observed — the intent contract deliberately settles R2b ("fresh by construction") and the wire `custom_id` plus defaults is the only testable surface; the ring bound is handled by the patch above.
  - `[false]` `[reject]` (intent 2) by-Ref Refs widen the AC1 exception and the test covers one Command path — the contract's Never list deliberately keeps by-Ref minting per send and documents it, and R6 fixes the test surface.
  - `[false]` `[reject]` (intent 3) mention neutralizing is render-only, Markdown is formatting-only, tests are per-case not one table, MSG-05 opt-in checked at `create_body` — all deliberately settled by the contract (E/M matrices, S1/S2 over `create_body`, "allowed_mentions still decides").
  - `[false]` `[reject]` (intent 4) CID-02 names the first path in a note and now reports unresolved ids — C1/C2 deliberately settle the note form and the Route + Payload comparison of unresolved ids.

## Design Notes

- **Why guarantee freshness.** The client keys a select's local state by its `custom_id`. With a random nonce, 1 send in 3844 would silently keep the old selection, so we avoid the nonce the client holds. Only the select that produced an interaction can hold a local selection, but a button click re-render must also avoid the message's current nonce, because an earlier select pick may have been answered without a re-render. So the scan covers `data` and `message`. Every select of one send shares one nonce, so in practice the set holds 1-2 entries, and the cap of 8 is generous.
- **Why scan the raw bytes.** It is zero-allocation and safe: inside a JSON string a quote is always escaped, so an unescaped `"custom_id"` can only be a key (N7). `twilight` may not parse V2 trees, and `serde_json::Value` would allocate on every dispatch.
- **Unresolved CID-02 key.** The placeholder nonce and signer make two equal Route values encode equally without an App key. The real ids also collide after resolution: one nonce per send, equal slots share one Ref, and the tag is deterministic. So the verdict is the same before and after sending.
- **Escape semantics.** Backslash escapes are what Discord renders literally. Escaping `[`/`]` is enough to kill masked links, and line-start markers are only special at a line start, so mid-line text stays clean (M11). Mentions use U+200B, not `\`, because `escape_markdown` would double a backslash and re-arm the mention.

```rust
let name = escape_markdown(&p.name);
let bio = p.bio.as_deref().map_or_else(|| format!("-# {}", s.profile.no_bio()), |b| escape_mentions(&escape_markdown(b)));
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (incl. trybuild with the re-blessed CID-02 `.stderr`, the insta snapshot, proptest, dhat `alloc_dispatch`/`memory_flat`)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt` (install nightly-2026-10-15 + cargo-public-api 0.52.0 first if missing, per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- ok
- `grep -rn "Story 8.2" examples/project-template crates docs` -- no `[planned` hit
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged

## Auto Run Result

- **Summary:** Story 8.2 makes renders deterministic and select menus fresh: each send with a select gets one new nonce that avoids the source interaction's nonces (scanned zero-allocation from raw `data`/`message` bytes) and the run's last eight sends; CID-02 now compares unresolved ids by Route + Payload; new `escape_markdown` / `escape_mentions` (U+200B) in `noctis-ui`, re-exported from `noctis`; profile template view uses them.
- **Files changed:** `crates/noctis-core/src/responder/nonce.rs` (nonce scan, avoid sets, draws); `responder.rs` (wiring, draw only for select ids); `responder/tests/components.rs` (deterministic FR-25 tests, scripted fill); `noctis-ui/src/escape.rs` (escapes); `noctis-ui/src/route.rs`, `limits/mod.rs`, `tests/validate.rs` (CID-02 unresolved keying); `noctis-macros/src/components/lower.rs`; `noctis-testkit/tests/select_freshness.rs` + snapshot; `noctis/src/lib.rs`, `noctis/tests/escape.rs`; CID-02 trybuild `.stderr`; `docs/discord-rules.md`; ARCHITECTURE-SPINE; `examples/project-template/` (AGENTS.md, noctis skill refs, profile/dashboard views); `xtask/public-api.txt`.
- **Review findings:** 26 total (high 0, medium 6, low 10, false 9, maybe-false 1).
  - Patched (medium 6, low 3): interaction nonces kept in a separate never-written set beside the 8-slot draw ring (5 duplicate findings), test >8 draws, docs reworded to the bounded guarantee; deterministic avoid-set tests with a `#[cfg(test)]` scripted fill; draw a nonce only for `has_select()` ids; escape docs list what stays live; re-wrapped `limits/mod.rs` docs.
  - Deferred: none.
  - Rejected: resolved+unresolved equal select ids bypass pre-send CID-02 (needs hand-resolve, x2); duplicated CID-02 help literal (deliberate, pinned by tests); `<id:customize>` links unescaped (ping nobody); lone `\r`/U+2028 line starts (unverifiable, low); template names skip `escape_mentions` (false, per guidance); `duplicate_key` skips failing encodes (settled C6); noctis-rules text (false, ids only); Mutex across getrandom (never across await); testkit sleeps (copied helpers per spec); identical-JSON test weakness (caught elsewhere); pre-resolved select carrying source nonce (needs hand-resolve); missing `message` (false); intent 1-4 (settled by contract).
- **Follow-up review recommended:** true (first pass, 6 medium patched, 0 high). Unverified risk: the reworked nonce avoid-set split (separate interaction set vs 8-slot draw ring) has not been independently reviewed.
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests incl. trybuild/insta/proptest/dhat, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** freshness guarantee is bounded to the interaction's nonces and the run's last eight sends; hand-resolved ids can evade pre-send CID-02; guild navigation links and exotic line breaks are not escaped.
