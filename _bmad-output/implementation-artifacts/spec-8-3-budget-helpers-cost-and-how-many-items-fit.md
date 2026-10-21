---
title: 'Story 8.3: Budget helpers: cost and "how many items fit"'
type: 'feature'
created: '2026-10-21'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
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
