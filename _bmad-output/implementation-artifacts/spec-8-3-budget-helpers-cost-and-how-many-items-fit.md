---
title: 'Story 8.3: Budget helpers: cost and "how many items fit"'
type: 'feature'
created: '2026-10-21'
status: 'done'
baseline_revision: 'b4f0fc59ec4d845205efe7dcf5c1d482ea72b8a5'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-8-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred:
  - summary: >-
      The template's worst-case page-line sizing in the Roles tab (`page_line(s, u16::MAX - 1, u16::MAX)` before `Budget::of(..).fits(..)`) is not verified by any test that runs in a local gate.
    evidence: |-
      Verification-gap layer (pre-verified): `examples/project-template` is excluded from the workspace (`Cargo.toml` `exclude`), so `cargo test --workspace` never builds `tests/flows.rs`; `skill-check` only lints its text; the CI `template-build` job only builds and is `continue-on-error` until Story 14.2. `flows.rs` also calls an undefined `roles_named` and checks `text_chars <= 4000` on page 1 only, with 120 roles (about 8 pages, one-digit page line). `pages_300_items_within_limits` passes with or without the worst-case measurement. Settle when the template test harness lands (Story 14.2): seed enough `ROLE_ROW_MAX_CHARS` roles for 10+ pages and assert the text budget on every page.
    location: >-
      examples/project-template/src/modules/dashboard/view.rs:112-119; examples/project-template/tests/flows.rs:90-112
    severity: medium
---

<intent-contract>

## Intent

**Problem:** FR-62 promises a budget helper that answers "how many more items of this shape fit beside the fixed parts of my message", so a render function can size a page slice by hand (UX-DR6, AD-27). Only `noctis::ui::cost` exists. `Budget` and `ctx.limits()` are already used by the template and the Usage Skill, marked `[planned]`, so neither builds against the real API.

**Approach:**
- Add `noctis_ui::Budget` (re-exported as `noctis::ui::Budget`): `Budget::of(&V2Message, &Limits)`, `components_left()`, `text_left()`, `fits(&item)` and `fits_prefix(items)`. They count CV2-03 (components at every depth) and CV2-04 (Text Display `discord_len`) against the configurable `Limits`, using the same lowering and `noctis_rules::component::cost` that `cost` uses.
- Add `ctx.limits() -> &Limits` to `CommandCtx`, `ComponentCtx` and `ModalSubmitCtx`.
- Prove the pagination contract with tests: a property test, a test that pages 300 items, and FR-24 send tests. Then reconcile the template, the skill, the docs and the API snapshot.

## Boundaries & Constraints

**Always:**
- `Budget` is `#[derive(Debug, Clone, Copy, PartialEq, Eq)]` with private fields `components_left: usize` and `text_left: usize`. It holds no reference to the message or the limits.
- `Budget::of` reads `limits.components` and `limits.text_total_len`, never hard-coded 40/4000. Both subtractions saturate at 0. It takes `&V2Message` and never changes, validates, truncates or logs. Lenient and Strict give the same numbers.
- An item's cost is exactly `noctis_ui::cost(&V2Message::new(Components::new().push(item.clone())), limits)`, read as `(components, text_chars)`. Placed in a Container it costs the same, because the Container is part of the fixed parts. The cost is computed by a `pub(crate)` helper in `limits/mod.rs` that reuses `lower::with_v2` and `rc::cost`.
- `fits(&item)` = `min(components_left / c, text_left / t)`, where an axis with cost 0 is unbounded on that axis. Every Container child costs at least 1 component, so the result is always finite.
- `fits_prefix(items)` is greedy and in order. It keeps a running sum (saturating) and stops at the first item that does not fit, even when a later item would fit. It pulls at most `k + 1` items from the iterator.
- Signatures:
  - `pub fn of(message: &V2Message, limits: &Limits) -> Self`
  - `pub fn components_left(&self) -> usize`
  - `pub fn text_left(&self) -> usize`
  - `pub fn fits<C: ContainerChild + Clone>(&self, item: &C) -> usize`
  - `pub fn fits_prefix<'a, C, I>(&self, items: I) -> usize where C: ContainerChild + Clone + 'a, I: IntoIterator<Item = &'a C>`

  All of them are `#[must_use]`. They never panic and use no `unwrap`/`expect` outside tests.
- `ctx.limits()` returns `&AppShared.limits` when the context has an App. Otherwise it returns a `static DEFAULT_LIMITS: std::sync::LazyLock<Limits>`.
- Release-only builds; trybuild stays `compile_fail` only.

**Never:**
- Noctis never paginates, splits or drops components, and never truncates anything other than Lenient text (FR-24 unchanged).
- No new rule ids, no new Limits fields and no `noctis-rules` change. No new crates or dependencies.
- `fits` does not check per-field rules (labels, URLs, CV2-06 shape) or CID-02. Its docs say so: n copies of one Route button with the same Payload fail CID-02, so real rows differ by Payload.
- No `Budget` for `LegacyMessage` or `Modal`. No testkit API (Story 8.4) and no new recipes example (Story 8.5).

## I/O & Edge-Case Matrix

Default `Limits` unless stated. `T(s)` = `TextDisplay::new(s)`, `L` = `LinkButton::new("https://e.com")`, `x×n` = `"x".repeat(n)`.

**`Budget::of` → `(components_left, text_left)`**

| # | Message | Result |
|---|---|---|
| B1 | `V2Message::new(Components::new())` (empty) | `(40, 4000)` |
| B2 | top-level `T("hi")` | `(39, 3998)` |
| B3 | `Container[T("## T"), ActionRow::buttons([L,L,L]), Separator::small()]` | `(33, 3996)` (1+1+4+1 components) |
| B4 | `Section::new([T("a"),T("b"),T("c")], L)` | `(35, 3997)` (Section + 3 texts + accessory) |
| B5 | `T("😀é")` | `(39, 3997)` (`discord_len` 3: emoji 2 UTF-16 units) |
| B6 | 41 × `T("ab")` (already over CV2-03) | `(0, 3918)`, no panic |
| B7 | `T(x×4001)` | `(39, 0)` |
| B8 | `ActionRow::select(StringSelect)` with 25 options; `MediaGallery` with 10 items | each costs 2 and 1 (options and gallery items do not count) |
| B9 | B3 with `limits.components = 10, text_total_len = 100` | `(3, 96)` |
| B10 | B3 with `limits.components = 5` (below the cost) | `(0, 3996)` |
| B11 | B3 with `LimitMode::Lenient` | same as B3 |

**`fits(&item)`** (in-module unit tests may build the `Budget` with the given remainder directly; the fields are private)

| # | Remainder | Item | Result |
|---|---|---|---|
| F1 | (40, 4000) | `T("")` | 40 (text axis unbounded) |
| F2 | (33, 3996) | `T(x×230)` | 17 (text binds: 3996/230) |
| F3 | (33, 3996) | `Section::new([T(x×10)], L)` (cost 3, 10) | 11 (components bind) |
| F4 | (33, 3996) | `ActionRow::buttons([L;5])` (cost 6, 0) | 5 |
| F5 | (6, 4000) / (5, 4000) | the F4 row | 1 / 0 (exact fill / one short) |
| F6 | (0, 4000) | `T("a")` | 0 |
| F7 | (12, 0) | `T("a")` / `Separator::small()` | 0 / 12 |
| F8 | (40, 460) / (40, 459) | `T(x×230)` | 2 / 1 (text boundary) |
| F9 | (40, 4000) | `T("😀"×100)` (text cost 200) | 20 |
| F10 | (40, 4000) | `T(x×4001)` | 0 |
| F11 | (40, 4000) | `ContainerComponent::from(T(x×100))` | 40 (`ContainerComponent` accepted) |
| F12 | (40, 4000) | `ActionRow::buttons([L.label(x×81)])` (cost 2, 0) | 20 (per-field CV2-08 is not counted; the send still fails CV2-08) |

**`fits_prefix(items)`** (component costs shown)

| # | Remainder | Items | Result |
|---|---|---|---|
| P1 | (40, 4000) | empty | 0 |
| P2 | (40, 4000) | 10 × `T("a")` | 10 |
| P3 | (7, 4000) | 3 Sections (cost 3 each) | 2 |
| P4 | (5, 4000) | costs `[1, 6, 1]` (`T("a")`, `ActionRow::buttons([L;5])`, `T("a")`) as `ContainerComponent`s | 1 (stops at the first miss, although the third would fit) |
| P5 | (40, 500) | `T(x×200)`, `T(x×200)`, `T(x×200)` | 2 (text binds mid-list) |
| P6 | (6, 4000) | costs `[3, 3]` | 2 (exact fill counts) |
| P7 | (3, 4000) | an iterator of 100 `T("a")` with an `inspect` counter | 3, and the counter is ≤ 4 |
| P8 | (0, 4000) | 5 items | 0, counter ≤ 1 |

**Properties** (`crates/noctis/tests/budget.rs`, `proptest`, 256 cases)

- Q1, `fits`. The fixed part is a valid Container with 1..=8 random children; `prop_assume!` that it validates. The item is random. With `n = fits(item)`:
  - the fixed Container with `n` copies pushed validates (Strict);
  - with `n + 1` copies it fails, and the violations include CV2-03 or CV2-04.

  Run it under default Limits and under random `components` 1..=40 and `text_total_len` 0..=4000.
- Q2, `fits_prefix`. Over a random list of 0..=30 items, with `k = fits_prefix(..)`: the first `k` validate; if `k < len`, the first `k + 1` fail with CV2-03 or CV2-04.
- Q3. `fits(item) == fits_prefix(std::iter::repeat_n(&item, 64))` whenever `fits(item) < 64`.
- The random-item strategy:
  - `T` with 0..=600 chars from `"[a-z ]|😀|é"`;
  - a Section with 1..=3 such texts and an `L` accessory with a label of ≤ 80 chars, or a Thumbnail accessory;
  - `ActionRow::buttons` with 1..=5 `L`;
  - `Separator`;
  - a `MediaGallery` with 1..=10 URL items.

  No Route ids, so CID-02 cannot fire.

**Pagination over 300 items** (`crates/noctis/tests/budget.rs`)

| # | Scenario | Expected |
|---|---|---|
| G1 | 300 role rows (names 1..=100 chars, every 7th containing `😀`, all escaped). The page is sized with `fits` on a 230-char worst-case sample. The fixed parts are a header, a 3-`L` tab row, a Separator, a worst-case page line and the pager | every page validates in Strict mode |
| G2 | Walk from page 0 by following the next button's target page | visits every page once; the concatenated rows are exactly the 300 in order |
| G3 | The first and the last page | prev is `disabled: true` on page 0, next on the last; the targets differ, so CID-02 passes |
| G4 | Walk back from the last page by following prev | reaches page 0 |
| G5 | Render with `page = u16::MAX` | clamps to the last page |

**FR-24 send** (`crates/noctis-core/src/responder/tests.rs`)

| # | Scenario | Expected |
|---|---|---|
| V1 | Strict, `texts(41)`, `reply` | `Invalid(UiError::Limits)` with CV2-03, path `components`, `Bound::AtMost(40)`, `Actual::Count(41)`; no request |
| V2 | Lenient, `texts(41)` | the same `Invalid` CV2-03; no request (never split) |
| V3 | Lenient, one `T(x×4500)` | one request whose body holds exactly one Text Display of `discord_len` 4000 ending `…` (truncated, not split) |

**`ctx.limits()`**

| # | Scenario | Expected |
|---|---|---|
| C1 | A command, component and modal-submit handler on an App built with `.limits(l)` where `l.components = 30` | each `ctx.limits() == &l` |
| C2 | A detached context (`__testing` seam) | `&Limits::default()` |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/limits/mod.rs`:
  - L14-16: module list. Add `mod budget;` and `pub use budget::Budget;`.
  - L248 `cost`, and L284-293 `V2Message::cost_with`: the pattern to reuse.
  - Add `pub(crate) fn child_cost(child: &ContainerComponent) -> (usize, usize)`. It builds a one-item `Components` (`items: vec![TopLevelComponent::from(child.clone())]`; `From<ContainerComponent>` is at `component/layout.rs` L499), calls `lower::with_v2(.., |t| rc::cost(t, &Limits::default()))` and returns `(c.components, c.text_len)`. `rc::cost` ignores its limits (`noctis-rules/src/component.rs` L570).
- `crates/noctis-ui/src/limits/budget.rs` (new): `Budget`. Rustdoc gives the formula, the "budgets only (CV2-03, CV2-04), not per-field rules or CID-02" caveat and a runnable example (B3 + F2). In-file `#[cfg(test)] mod tests` covers B1-B11, F1-F12 and P1-P8, named `budget_of_<case>`, `fits_<case>` and `fits_prefix_<case>`.
- `crates/noctis-ui/src/placement.rs`:
  - L211-220: the `container_child!` macro gives `sealed::IntoChild::into_child(self)`;
  - L357-365: the list, which already includes `ContainerComponent`.

  Inside the crate, call `item.clone().into_child()`.
- `crates/noctis-ui/src/lib.rs` L251: `pub use limits::{Budget, CostReport, ..}`. Update the crate docs near L188.
- `crates/noctis/src/lib.rs`:
  - L1905+: the `pub mod ui` docs. Add a "Budgets" bullet with a doctest (`Budget::of(..).fits(..)`).
  - L2457-2471: the re-export list. Add `Budget`.
- `crates/noctis-core/src/component.rs` L68-150, the `ctx_accessors!` macro (used by `ComponentCtx` and `ModalSubmitCtx`): add `pub fn limits(&self) -> &noctis_rules::Limits`. `crates/noctis-core/src/command.rs` L220-290, the `CommandCtx` accessors: add the same. Put the `static DEFAULT_LIMITS` in one place, `command.rs` as `pub(crate)`. `AppShared.limits` is read by `App::limits` (`app.rs` L273).
- `crates/noctis-core/src/responder/tests.rs` L381-430: the `long_label`/`texts` helpers, `limits_strict_and_lenient` and `Fixture::start_with(discord(), limits)`. Add `fr_24_lenient_never_splits` (V1-V3).
- `crates/noctis/tests/guards.rs` L68-80 and `escape.rs` L1-30: patterns for a `#[derive(Button)]` with a `#[handler]` impl (CID-04) and for a proptest file.
- Template, under `examples/project-template/`:
  - `src/modules/dashboard/view.rs` L108-117: drop `[planned, Story 8.3]`, and measure `fixed` with `page_line(s, u16::MAX - 1, u16::MAX)` (the worst case);
  - `src/modules/dashboard/handlers.rs` L51/85/96/110: already call `ctx.limits()`;
  - `tests/flows.rs` L90: already asserts the pages; leave it unchanged.
- Skill, under `examples/project-template/.claude/skills/noctis/`:
  - `references/views-and-recipes.md`: L30 (`ctx.limits()` [planned]), L158 and the §3.2 `fixed` line (worst-case page line), and §4 at L517-541 (drop `[planned, Story 8.3]`; document the formula, the greedy `fits_prefix`, budgets-only and saturation);
  - `references/limits-cheatsheet.md` L235;
  - `references/components-v2.md` L50-51 and L854-870;
  - `references/i18n.md` L60;
  - `SKILL.md`: L137/L179 and the API hash at L23;
  - the context-accessor list in `references/app-and-sharding.md` / `responder.md` (`grep -rn "response_locale()" references`): add `limits()`.
- `docs/discord-rules.md` L191-192 (CV2-03, CV2-04): add "`noctis::ui::Budget` reports what is left beside fixed parts (FR-62)" and the new test names.
- Spine `ARCHITECTURE-SPINE.md` AD-27 (L1197): no change needed; the rule already names the budget helpers. The memlog already has the three Story 8.3 `(decision)` lines.

## Tasks & Acceptance

**Execution (in order):**
1. `crates/noctis-ui/src/limits/mod.rs` + `limits/budget.rs` (new) + `lib.rs`: add `child_cost`, `Budget`, its docs and the unit tests B1-B11, F1-F12 and P1-P8, plus the re-export.
2. `crates/noctis/src/lib.rs`: the re-export and the `ui` docs bullet with a doctest.
3. `crates/noctis-core/src/command.rs` + `component.rs`: `limits()` on the three contexts, plus the default static. Rustdoc: "the App's Limits (`AppBuilder::limits`), what the Responder validates against; pass it to render functions that size pages with `Budget`". Test C1 in `crates/noctis-testkit/tests/responder_ergonomics.rs`, or the closest testkit file that starts an App with `.limits(..)`; test C2 as a unit test.
4. `crates/noctis-core/src/responder/tests.rs`: `fr_24_lenient_never_splits` (V1-V3).
5. `crates/noctis/tests/budget.rs` (new):
   - proptests Q1-Q3;
   - `pages_300_items_within_limits` (G1-G5), with `#[derive(Button)] #[button("t.page", defaults(style = Secondary, label = "Next"))] struct TurnPage { page: u16 }` and a `#[handler]` impl;
   - the pager's target pages are read by mapping each rendered `custom_id` (from `create_body` JSON) through a lookup built from `TurnPage { page: k }.build()` for `k` in `0..=pages`.
6. Template: the `view.rs` worst-case `fixed` line, and drop every `[planned, Story 8.3]` and `ctx.limits()` `[planned]` marker in the template and the skill.
7. Skill and docs, as in the Code Map. `views-and-recipes.md` §4 must show `Budget::of`, `components_left`, `text_left`, `fits` and `fits_prefix`, plus the rule "measure fixed parts with worst-case variable text". `limits-cheatsheet.md` must give the formula.
8. Run `cargo run --release -p xtask -- api-snapshot --bless` and put the printed hash on `SKILL.md`'s `API hash` line.
9. Run every Verification gate.

**Acceptance Criteria:**
- Given a partly built V2 message and the App's `Limits`, when `noctis::ui::Budget::of(&msg, &limits)` is called, then `components_left()` and `text_left()` equal the configured limits minus `noctis::ui::cost(&msg, &limits)`, saturating at 0 (B1-B11).
- Given a budget and a sample Container child, when `budget.fits(&item)` is called, then `n` copies beside the fixed parts validate and `n + 1` fail with CV2-03 or CV2-04 (Q1). `fits_prefix` returns the greedy in-order count with the same property (Q2).
- Given 300 items paged with `fits` and prev/next Route buttons, when every page is rendered and validated, then no page exceeds a Limit and following next (and back with prev) reaches every item exactly once (G1-G5).
- Given an over-Limit message sent through the Responder, when it is sent in Strict or Lenient mode, then it is `ResponderError::Invalid` naming the path, Limit and actual value with no request. The exception is Lenient over-long text, which is truncated and never split (V1-V3).
- Given a handler context, when `ctx.limits()` is called, then it returns the App's configured Limits (C1-C2).
- Given the new public API, when `skill-check` runs, then it passes: `Budget` is documented in `views-and-recipes.md` §4 and `limits-cheatsheet.md`, the template's dashboard Roles tab uses it, no `[planned, Story 8.3]` remains, and the `API hash` matches.

## Spec Change Log

## Review Triage Log

### 2026-10-21 — Review pass
- verdicts: 22 findings — high 0, medium 1, low 8, false 13, maybe-false 0
- findings:
  - `[low]` `[patch]` (blind) `fits_prefix` takes only `&'a C` items, so an owned lazy `.map(..)` does not compile while `views-and-recipes.md` §4 says "a lazy iterator over a long list is fine" — the signature is fixed by the spec, but the skill wording misleads authors into a compile error; patch: reword the skill line to say items are borrowed (`rows.iter()`, `.iter().skip(offset)`), build owned rows first.
  - `[low]` `[patch]` (blind) `fits`/`fits_prefix` clone each item twice (`item.clone().into_child()` then `child.clone()` inside `child_cost`) — real avoidable clone; patch: `child_cost` takes the `ContainerComponent` by value so the second clone goes (the public `Clone` bound stays, the spec's signature requires it).
  - `[false]` `[reject]` (blind) `child_cost` passes `Limits::default()` to `rc::cost`, so `fits` could disagree with `validate` if `rc::cost` ever read limits — `noctis-rules/src/component.rs` `cost` does `let _ = limits;`, the spec mandates this, and the Q1/Q2 proptests run under random Limits against `validate`, so any future divergence fails them.
  - `[false]` `[reject]` (blind) `fits` only takes `ContainerChild` items, top-level items cannot be sized — the intent (epics AC2 "any Container child") and spec Design Notes select this; rustdoc ("how many more Container children fit") and the skill ("The item is any Container child") state it.
  - `[low]` `[reject]` (blind) `limits()` exists only on Command/Component/ModalSubmit contexts; `EventCtx` has none — the intent asks for none, EventCtx sends through raw `http()` that Noctis does not validate, rarely met, and the fix adds public API surface.
  - `[low]` `[patch]` (blind) the three `limits()` accessors carry drifting rustdoc (`CommandCtx` links [`Limits::default`], the other two use plain backticks) — moving them into `ctx_accessors!` would also add `ExpiredCtx::limits` (new public surface), so patch only the doc text: make the two `component.rs` doc comments identical to `CommandCtx`'s.
  - `[false]` `[reject]` (blind) proptests filter with `prop_assume!` and raise `max_global_rejects` to 1,000,000, risking slowness and "too many global rejects" flakes — each reject costs one cheap validate, 256 cases need far fewer than 1,000,000 rejects, and the suite passes; the flake cannot occur at that budget.
  - `[low]` `[reject]` (blind) no test pins the CID-02 caveat, Lenient end to end through `ctx.limits()`, or dispatches `TurnPage::handle` — the caveat is documented and `fits_ignores_per_field_rules` pins the per-field half, C1 tests `ctx.limits()` through a running App; extra end-to-end tests are more than a direct correction for a negligible gap.
  - `[low]` `[patch]` (blind) `noctis-ui/src/lib.rs` module doc: after inserting the `Budget` clause, "Both lower a message tree" is ambiguous and wrong for `Budget` — patch: make the sentence name `validate` and `cost`.
  - `[low]` `[patch]` (blind) `docs/discord-rules.md` CV2-03/CV2-04 rows cite `fr_24_lenient_never_splits` and `limits::budget::tests` without crate paths, unlike the other cited tests — patch: give their paths (`crates/noctis-core/src/responder/tests.rs`, `crates/noctis-ui/src/limits/budget.rs`).
  - `[false]` `[reject]` (blind) test `render` underflows on an empty role list and `.max(1)` hides a zero budget — the test's role list is a fixed 300; `fits == 0` only when the fixed parts alone exhaust the Limits, and then the over-budget page fails loudly with the FR-24 `Invalid` error (correct behavior; without `.max(1)` the page count divides by zero). The spec's Design Notes prescribe `.max(1)`.
  - `[false]` `[reject]` (edge) template `.max(1)` forces one row when fixed parts exhaust the budget — same refutation: the Responder rejects that page with a typed CV2-03/CV2-04 error and no request; the proposed "no roles" fallback would hide a real misconfiguration.
  - `[medium]` `[defer]` (vgap, pre-verified) the template's worst-case page-line sizing is verified by no test that runs (template excluded from the workspace, `flows.rs` non-compiling and checking page 1 text only) — filed disposition defer: the template test harness lands with Story 14.2; recorded in `deferred`.
  - `[false]` `[reject]` (intent 1) the 300-item test steps pages by mapping `custom_id`s, not by dispatching clicks — the spec settled G2 this way, it decodes real rendered `custom_id`s against `TurnPage { page: k }.build()`, and `ctx.limits()` through dispatch is C1.
  - `[false]` `[reject]` (intent 2) AC5 "the template's Roles tab uses it" is asserted only by pre-existing code — the AC is satisfied: the Roles tab calls `Budget::of` and `ctx.limits()`, now real API, and `skill-check` passes; the test gap is the deferred vgap entry.
  - `[false]` `[reject]` (intent 3) `Budget::of` takes only `V2Message`, not a `Components`/`Container` or legacy message — spec Design Notes settle this correctly (Container children exist only in V2; legacy text is MSG-01); wrapping fixed parts in `V2Message::new` is the partly built message.
  - `[false]` `[reject]` (intent 4) `fits_prefix` accepts only an iterator of references — an iterator of `&ContainerComponent` is "an iterator of differently sized items"; the spec fixes this signature. The misleading skill wording is the patched blind row.
  - `[low]` `[reject]` (intent 5) the proptest generator omits selects, `File` and Route buttons — CID-02 exclusion is deliberate; select-row cost is pinned by B8, and the budget arithmetic is shape-independent; adding generators is more than a direct correction for a negligible gap.
  - `[false]` `[reject]` (intent 6) CV2-06 is in the story's Implements line but not tied to `Budget` — CV2-06 is enforced at compile time by `ActionRow::buttons`/`select`; `Budget` counts the valid row shapes (B8, F4, F12), and the spec rightly keeps row shape out of the budget.
  - `[false]` `[reject]` (intent 7) no public per-component cost (FR-62 consequence 1) — the FR traceability row assigns `cost` to Story 4.3, and any component's cost is `cost(&V2Message::new(Components::new().push(item)), &limits)`, exactly how the spec defines item cost.
  - `[false]` `[reject]` (intent 8) FR-24 proof covers only `reply` — the spec's V1-V3 settle this; every send path builds its body through the same validation, already covered by `limits_strict_and_lenient` and earlier stories.
  - `[false]` `[reject]` (intent 9) `ctx.limits()` is public surface the story text does not name — the intent-contract Approach names it, and it is the only way a handler's render function gets "the App's Limits" the AC requires.

## Design Notes

- **Why only `V2Message`.** Sample items are Container children, which exist only in V2. A legacy message's text budget is `content` (MSG-01), a different Limit.
- **Why the `ContainerChild + Clone` bound.** One method covers `TextDisplay`, `Section`, `ActionRow`, … and mixed lists as `ContainerComponent`. The clone costs one allocation per call, off the dispatch path, so it does not touch the `alloc_dispatch` budget.
- **Worst-case fixed parts.** Measuring the fixed parts with "Page 1 of 1" while sending "Page 12 of 13" can push a text-bound page over CV2-04 by a few characters. Measure every variable fixed part at its largest.

```rust
let fixed = frame(header.clone(), tabs.clone(), vec![page_line(s, u16::MAX - 1, u16::MAX).into(), pager(d, s, 0, 1).into()]);
let per_page = Budget::of(&fixed, limits).fits(&TextDisplay::new("x".repeat(ROLE_ROW_MAX_CHARS))).max(1);
```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (budget unit tests, `crates/noctis/tests/budget.rs` proptests, `fr_24_lenient_never_splits`, doctests, dhat `alloc_dispatch`/`memory_flat`)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok
- `cargo run --release -p xtask -- rules-catalog` -- ok
- `cargo run --release -p xtask -- api-snapshot` -- matches the re-blessed `xtask/public-api.txt` (if missing, first install nightly-2026-10-15 and cargo-public-api 0.52.0 per AGENTS.md)
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `cargo metadata --format-version 1 --manifest-path examples/project-template/Cargo.toml > /dev/null` -- ok
- `grep -rn "planned, Story 8.3" examples/project-template crates docs` -- no hit
- `grep -rn "ctx.limits()" examples/project-template | grep planned` -- no hit
- CI: no new gate; `.github/workflows-disabled/ci.yml` unchanged

## Auto Run Result

- **Summary:** Added `noctis_ui::limits::Budget` (`Budget::of(&V2Message, &Limits)`, `fits(&item)`, `fits_prefix(iter)`) answering how many more Container children fit under CV2-03/CV2-04 using the same `cost` as `validate`; added `ctx.limits()` on Command/Component/ModalSubmit contexts; FR-24 proof that Lenient never splits; template Roles tab paginates with `Budget` sized at worst-case fixed parts.
- **Files changed:** `crates/noctis-ui/src/limits/budget.rs` (Budget + unit tests), `crates/noctis-ui/src/limits/mod.rs` and `lib.rs` (exports, docs), `crates/noctis-core/src/command.rs`/`component.rs` (`limits()` accessors), `crates/noctis-core/src/responder/tests.rs` (`fr_24_lenient_never_splits`), `crates/noctis/src/lib.rs` (re-exports), `crates/noctis/tests/budget.rs` (proptests vs `validate`, 300-item pagination), `crates/noctis-testkit/tests/responder_ergonomics.rs` (`ctx.limits()` via App), `docs/discord-rules.md` (CV2-03/04 rows), `xtask/public-api.txt` (re-blessed), template skill refs, `AGENTS.md`, `dashboard/view.rs`.
- **Review findings (22):** high 0, medium 1, low 8, false 13. Patched 5 (all low): skill wording on borrowed items for `fits_prefix`, removed double clone in `child_cost`, unified `limits()` rustdoc, clarified `noctis-ui` module doc, crate paths for cited tests in discord-rules. Deferred 1 (medium): template worst-case page-line sizing untested until the template harness (Story 14.2). Rejected 16 with recorded reasons (e.g. `Limits::default()` in `child_cost` is unused by `rc::cost` and proptests guard divergence; `.max(1)` is spec-prescribed and over-budget pages fail with typed FR-24 error; no `EventCtx::limits` to avoid new surface; CID-02 exclusion from generators deliberate).
- **Follow-up review recommended:** false (patched: high 0, medium 0, low 5).
- **Verification:** local gates via `tools/story-loop/gates.sh` all green (fmt, clippy -D warnings, workspace tests, cargo-hack each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91); CI paused by maintainer.
- **Residual risks:** template Roles-tab pagination is not exercised by a running test until Story 14.2; `Budget` ignores per-field rules (documented CID-02 caveat).
