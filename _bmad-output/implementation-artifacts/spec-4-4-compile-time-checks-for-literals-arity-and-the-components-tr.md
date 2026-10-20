---
title: 'Story 4.4: Compile-time checks for literals, arity and the `components!` tree macro'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: 'fc56965d9bcf5fca782e2b06ce7c5eb792d32377'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/components-v2.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Static mistakes in Components V2 messages still reach runtime. Array constructors are unbounded in `N` (empty or oversized rows, Sections and galleries compile). Literal labels, placeholders, legacy content and colours are unchecked until `validate`. There is no `components!` macro, so a fixed-shape tree with 41 components, 4001 literal characters or a duplicate literal Route component compiles (FR-75, FR-78, NFR-10; AD-3, AD-4, AD-12). The rules CID-02, CV2-03, CV2-04, CV2-06, CV2-08..CV2-11, CV2-13, CV2-17 and MSG-01 each still have a `diagnosed` entry for this story in `xtask/rules-pending.toml`.

**Approach:**
- **Arity (`noctis-ui`):** add sealed arity traits on the existing `Arity<N>` (`RowButtons`, `SectionTexts`, `GalleryItems`) with `#[diagnostic::on_unimplemented("[CV2-0x] …")]`. They bound `ActionRow::buttons`, `Section::new` and `MediaGallery::new`. Add fallible `try_*` constructors for runtime lengths.
- **Literal newtypes (`noctis-ui`):** add `Label`, `Placeholder` and `Content`, each with a `try_new`/`TryFrom` runtime path, and add `Rgb::try_from(u32)`. All of them call the `noctis-rules` function that the macros call.
- **Macros (`noctis-macros`):**
  - `label!`, `placeholder!`, `content!` and `rgb!` measure their literal at expansion.
  - `components!` parses the story grammar and lowers the tree into `noctis_rules::component` descriptors. It calls `validate_components` and emits each violation as a span-precise `compile_error!`, adds the CID-02 check, and otherwise expands to typed builder calls under `::noctis::__private::ui`.
- **Facade, tests and docs:** re-export everything through the facade. Add the trybuild fixtures and the boundary passes, empty the 4.4 entries of `rules-pending.toml`, and update the catalog How text, the skill and the template.

## Boundaries & Constraints

**Always:**
- **Limits from `noctis-rules` only.** Every limit comes from a `noctis-rules` function called with `&Limits` (the macros use `Limits::default()`). No limit constant is copied into `noctis-macros` or `noctis-ui`. `discord_len` is the only length function. The macro, the newtype `try_*` path and the runtime validator use the same function for the same rule:

  | Rule | Function |
  | --- | --- |
  | CV2-08 label | `validate_button_label` |
  | CV2-08 link URL | `validate_link_url` |
  | CV2-09 placeholder | `validate_select_placeholder` |
  | MSG-01 | `message::validate_content` |
  | CV2-13 | `validate_accent_color` |
  | CV2-10 / CV2-11 alt | `validate_gallery_description` / `validate_thumbnail_description` |
  | CV2-17 | `validate_media_url` |
  | counts | new `validate_row_buttons` / `validate_section_texts` / `validate_gallery_items` |
  | trees | `validate_components` |
- **AD-4 mechanisms only.** Use proc-macro errors, sealed traits with `on_unimplemented`, or construction. Never a `const` assert in generic code. Every check fails under `cargo check`.
- **Macro output paths.** Generated code names runtime items only as `::noctis::__private::ui::…` and `::core`/`::std`. Errors are spanned on the offending token. A rule violation's message is the `Violation`'s `Display` (it starts with `[<RULE-ID>]` and carries its `= note:` and `= help:` lines).
- **Arity traits** are implemented only for the legal `Arity<N>`: `RowButtons` for 1..=5, `SectionTexts` for 1..=3, `GalleryItems` for 1..=10. Each uses the existing `placement::sealed::Sealed` supertrait, like `LegacyRows`. The messages:
  - `[CV2-06] an Action Row holds 1 to 5 buttons`
  - `[CV2-07] a Section holds 1 to 3 Text Displays`
  - `[CV2-10] a Media Gallery holds 1 to 10 items`

  Each has a label, and a note that names `` `{Self}` `` and the `try_*` runtime path.
- **New public items keep the 4.1-4.3 conventions:** `Debug + Clone + Send + Sync`, rustdoc with a runnable doctest each, every item named in the Usage Skill, `xtask/public-api.txt` re-blessed and the new hash written into `SKILL.md`.
- **Dependencies.** No new external crate. In `crates/noctis-macros/Cargo.toml` change only `syn` to `syn = { workspace = true, features = ["full"] }` (syn 3.0.9, already in `Cargo.lock`). `noctis-macros` already depends on `noctis-rules` (allowed edge, `xtask/src/deps_check.rs` L62).
- **Release-only commands.** Use `--release` for every cargo command (trybuild is covered by `.cargo/config.toml`). Never edit `.cargo/config.toml` or `_bmad/`. CI is paused, so local gates are the whole check.

**Never:**
- Do not change the semantics of the existing `noctis_rules::component` checks or the descriptor types. The only `noctis-rules` change is the three new count functions plus their tests.
- Do not add any of these:
  - a string `custom_id` constructor
  - `try_string_select`
  - `button_with` (Story 9.3)
  - an `alt!`/`lit!` macro
  - modal macros (4.5/4.6)
  - `View::render` (Epic 8)
  - raw components (9.2)
- Do not relax the runtime validator. It still re-checks every count, because `Limits` may be lowered.
- No `unsafe`, no arena crate, no new dev-dependency.

## I/O & Edge-Case Matrix

Notation:
- `L` = `Limits::default()`.
- `B(n)` = `Press { n }.button()` for a `#[derive(Route)] #[route("t.press")] struct Press { n: u8 }` with `Handled`.
- `P` = `Pick` (a `select` Route) and `U` = `Refresh` (a unit Route).
- `"a"*n` = `"a".repeat(n)` written as a literal. `😀*n` = n emoji (2 units each).
- "CE[R @ span]" = the macro or rustc fails `cargo check` with `[R]` in the message, spanned as stated.
- "Err[R …]" = `Err(UiError::Limits { violations })` whose violation has rule R, an empty path (`content` for MSG-01) and the given bound and actual.

**Literal macros and newtypes**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 1 | label max | `label!("a"*80)`, `label!(😀*40)`, `label!("")` | compile, `Label` with that text | none |
| 2 | label max+1 | `label!("a"*81)`, `label!(😀*41)` | CE[CV2-08 @ literal]: `limit at most 80, actual 81` (82) | compile error |
| 3 | label escapes, raw | `label!("\u{1F600}")`, `label!(r"x")` | measured on the unescaped value (2 / 1) | none |
| 4 | label misuse | `label!(name)`, `label!(1)`, `label!(b"x")`, `label!()`, `label!("a", "b")` | CE[CV2-08 @ argument] "label! takes one string literal", help `Label::try_new(text, &limits)` | compile error |
| 5 | label const | `const SAVE: Label = label!("Save");` | compiles; `.label(SAVE)` on `RouteButton`/`LinkButton` works (`From<Label> for Cow<'static, str>`) | none |
| 6 | Label runtime | `Label::try_from("a"*80)` ok; `Label::try_from("a".repeat(81))` (`String`); `Label::try_new(s, &l)` with `l.button_label_len = 3` and `"abcd"` | Err[CV2-08 AtMost(80) Count(81)]; Err[CV2-08 AtMost(3) Count(4)]; never truncates, even in Lenient mode | `UiError::Limits` |
| 7 | placeholder | `placeholder!("a"*150)` ok; `"a"*151`; misuse as in row 4 | ok / CE[CV2-09 @ literal] Count(151) / CE[CV2-09] misuse; `Placeholder::try_from` / `try_new` mirror row 6 with AtMost(150) | as stated |
| 8 | content | `content!("a"*2000)` ok; `"a"*2001`; `😀*1001`; `content!("")` ok | CE[MSG-01 @ literal] Count(2001/2002); `Content::try_from` / `try_new` Err path `content`; `LegacyMessage::new(content!("hi"))` compiles | as stated |
| 9 | rgb ok | `rgb!(0x000000)`, `rgb!(0)`, `rgb!(0xFFFFFF)`, `rgb!(0x5865_F2)`, `rgb!(16777215u32)`, in `const` | `Rgb` with that value (`== Rgb::new(255, 255, 255)` for 0xFFFFFF) | none |
| 10 | rgb overflow | `rgb!(0x1000000)`, `rgb!(0xFFFFFFFF)`, `rgb!(99999999999999999999)` | CE[CV2-13 @ literal] `limit a colour from 0x000000 to 0xFFFFFF, actual` `` `0x1000000` `` (unparsable u32: the literal's text) | compile error |
| 11 | rgb misuse | `rgb!(-1)`, `rgb!(1.0)`, `rgb!("x")`, `rgb!(v)`, `rgb!()` | CE[CV2-13] "rgb! takes one non-negative integer literal", help `Rgb::try_from(u32)` | compile error |
| 12 | Rgb runtime | `Rgb::try_from(0xFFFFFF)` ok; `0x1000000`; `u32::MAX` | Ok; Err[CV2-13 Describe(range) Text("0x1000000")] | `UiError::Limits` |

**Arity traits and `try_*` constructors**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 13 | row arity | `ActionRow::buttons` with 1 and 5 | compile | none |
| 14 | row arity out | `ActionRow::buttons::<Button, 0>([])`; 6 buttons | E0277 `[CV2-06] an Action Row holds 1 to 5 buttons`, help names `Arity<0>` / `Arity<6>` | compile error |
| 15 | section arity | `Section::new` with 1 and 3 texts ok; 0 and 4 texts | 0/4: E0277 `[CV2-07] a Section holds 1 to 3 Text Displays` | compile error |
| 16 | gallery arity | 1 and 10 items ok; 0 and 11 | 0/11: E0277 `[CV2-10] a Media Gallery holds 1 to 10 items` | compile error |
| 17 | try_buttons | `ActionRow::try_buttons(vec_of_5, &L)` ok; empty iterator; 6 items; `l.row_buttons = 2` with 3 | Ok; Err[CV2-06 Between{1,5} Count(0/6)]; Err Between{1,2} Count(3) | `UiError::Limits` |
| 18 | try_new section | `Section::try_new(texts, accessory, &L)` with 1..=3 ok; 0, 4 | Err[CV2-07 Between{1,3} Count(0/4)] | `UiError::Limits` |
| 19 | try_new gallery | `MediaGallery::try_new(items, &L)` with 1..=10 ok; 0, 11 | Err[CV2-10 Between{1,10} Count(0/11)] | `UiError::Limits` |
| 20 | rules counts | `validate_row_buttons(n, &L)` for n = 0, 1, 5, 6; `validate_section_texts` for 0, 1, 3, 4; `validate_gallery_items` for 0, 1, 10, 11 | 1/0/0/1 violations, path `""`, `Between{1, limit}`, `Count(n)` | n/a |
| 21 | validator still counts | `try_buttons` with `l.row_buttons = 6` and 6 buttons, then `validate(&mut msg, &L)` | Err CV2-06 at `row[0]` (Between{1,5} Count(6)); likewise Section 4 texts and gallery 11 items | error |

**`components!`: expansion**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 22 | expansion equals builders | tree `T1` (Design Notes) | `serde_json::to_value(components!{T1})` equals the hand-built builder tree, ids included | none |
| 23 | empty | `components! {}` | `Components::new()` (len 0; MSG-09 is the validator's) | none |
| 24 | text forms | `text("lit")`, `text(format!(..))`, `text(var)` | `TextDisplay::new(x)`; non-literal counts 0 chars | none |
| 25 | container | `container [ … ]`, `container(accent = rgb!(0x5865_F2)) [ … ]`, `container(accent = 0x5865F2) [ … ]`, `container(accent = ACCENT) []` | `Container::new().accent(..).push(..)…`; an integer literal is checked (CV2-13) and becomes `Rgb`; an empty container is allowed | none |
| 26 | separator | `separator`, `separator(spacing = large)`, `separator(divider = false, spacing = small)` | `Separator::new()` / `large()` / `small().divider(false)`; keys in any order, each at most once | duplicate or unknown key, or spacing not `small`/`large`: plain syn error (spacing: CE[CV2-13]) |
| 27 | thumbnail / item media | `thumbnail("https://x.y/a.png")`, `thumbnail(card_ref)`, `item(media, alt = "Card")`, `alt = alt_var` | a string literal becomes `Media::url(lit)`, anything else goes to `Thumbnail::new` / `MediaItem::new` (`impl Into<Media>`); `.alt(x)` | none |
| 28 | file | `file(card_ref)` | `File::new(card_ref)` (a non-`AttachmentRef` is rustc E0308) | rustc |
| 29 | row buttons | `row [ B(1), link(url = "https://x.y", label = "Docs"), premium(sku), {b} ]` | `ActionRow::buttons([row_button(..), …])`; a hole is one button | none |
| 30 | button args and setters | `Press{n:1}.button()`, `.button("Save")`, `.button(label!("Save"))`, `.button(name)`, `.button("Save").style(ButtonStyle::Primary)`, `.label("x")` setter | `RouteComponents::button(recv)` + `.label(arg)`, then the outer setters in order; a literal label arg or a `.label("lit")` setter is measured (CV2-08) | ≥ 2 args: syn error |
| 31 | selects | `P.string_select([SelectOption::new("a","a")])`, `P.user_select().placeholder("Pick").min_values(1).max_values(3)`, the other three kinds | `RouteComponents::<kind>(recv[, options])` + setters; literal `.placeholder` (CV2-09) and literal `min_values` / `max_values` integers are lowered and checked; an array-literal options argument is counted (CV2-09 1..=25) | wrong arg count: syn error |
| 32 | holes | `{node}` at top level, in a container, as an accessory (`accessory = {b}`) and in a row | pushed as-is through the placement-bounded builder (`Components::push`, `Container::push`, `Section` accessory, `row_button`); a misplaced hole is rustc E0277 `[CV2-05]` / `[CV2-07]` | rustc |
| 33 | spreads | `..iter` at top level and in a container | `for x in iter { c = c.push(x) }` (placement-bounded); counts 0 | none |
| 34 | spread in a row / section / gallery | `row [ ..buttons ]` | syn error "spreads go only at the top level or in a container; build a dynamic row with ActionRow::try_buttons and pass it as a {hole}" | compile error |
| 35 | evaluation order | expressions with side effects | each expression is evaluated exactly once, in source order | none |
| 36 | hygiene | user variables named `c`, `__noctis_c`; no `use` of the prelude | compiles (UFCS through `::noctis::__private::ui`) | none |
| 37 | usable as a value | `V2Message::new(components!{ text("hi") })` | compiles; `validate` Ok | none |

**`components!`: compile-time rules**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 38 | CV2-03 max | exactly 40 (Container + 39 texts; and 8 top-level `row`s × 4 buttons = 40) | compiles | none |
| 39 | CV2-03 max+1 | 41 `text("a")`; Container + 40 texts | CE[CV2-03] `components: limit at most 40, actual 41` spanned on the node that crosses 40, with one `= note: kind[i]: n` per top-level node | compile error |
| 40 | CV2-03 holes / spreads | 39 texts + `{x}` ok (40); 40 texts + `{x}` fails (41); 40 texts + `..xs` ok | as stated | compile error on 41 |
| 41 | CV2-03 accessory | Section of 3 texts + thumbnail counts 5 | 8 such Sections = 40 ok; 8 + 1 text = 41 fails | compile error |
| 42 | CV2-04 max | literal texts totalling exactly 4000 (`😀*2000` ok) | compiles | none |
| 43 | CV2-04 max+1 | 4001 literal chars across nodes (top, container and section texts) | CE[CV2-04] `components: limit at most 4000, actual 4001` spanned on the literal that crosses 4000 | compile error |
| 44 | CV2-04 non-literal | 4000 literal + `text(var)` | compiles (lower bound) | none |
| 45 | CV2-05 placement | `container [ container [] ]`; top-level `thumbnail(..)`; top-level `B(1)` / `link(..)`; `text(..)` inside `row`; `item(..)` outside `gallery` | CE[CV2-05] spanned on the misplaced node (path such as `container[0].container[0]`) | compile error |
| 46 | CV2-06 | `row []`; 6 buttons; `row [ P.string_select([..]), B(1) ]`; two selects | CE[CV2-06 @ row] Count(0/6), or the select-not-alone violation | compile error |
| 47 | CV2-07 | `section [ ; accessory = B(1) ]` (0 texts); 4 texts; `accessory = text("x")`; a non-`text` node among the texts | CE[CV2-07 @ section] (count via rules, others via the macro) | compile error |
| 48 | CV2-08 | `link(url = "", …)`, url `"h"*513` (512 ok), `label = "a"*81`, `.button("a"*81)`, `.label("a"*81)` | CE[CV2-08 @ that literal] | compile error |
| 49 | CV2-09 | `.placeholder("a"*151)`; `string_select([])`; 26 array elements; `min_values(26)`; `max_values(0)`; `min 3 max 2` | CE[CV2-09] spanned on the literal / argument / select | compile error |
| 50 | CV2-10 | `gallery []`; 11 items; `item(m, alt = "a"*1025)` (1024 ok) | CE[CV2-10 @ gallery / alt literal] | compile error |
| 51 | CV2-11 | `thumbnail(m, alt = "a"*1025)` (1024 ok) | CE[CV2-11 @ alt literal] | compile error |
| 52 | CV2-13 | `container(accent = 0x1000000) []` | CE[CV2-13 @ literal] | compile error |
| 53 | CV2-17 | `thumbnail("")`, `thumbnail("h"*2049)`, `item("h"*2049)` (2048 ok) | CE[CV2-17 @ url literal] | compile error |
| 54 | many errors | one tree breaking CV2-03, CV2-08 and CID-02 | every violation reported in one expansion, in `validate_components` order, then CID-02; the expansion is the errors plus an empty `Components` | compile error |

**`components!`: CID-02**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 55 | duplicate | `row [ Press{n:1}.button(), Press{n:1}.button("x") ]` | CE[CID-02] on the second receiver "duplicate custom_id: `Press { n : 1 }` is already used at row[0].button[0]", plus CE[CID-02] "first use of the duplicate custom_id" on the first receiver | compile error |
| 56 | across depth / kinds | same receiver as a Section accessory and in a container row; `U.button()` twice; `Page(1).button()` twice; button and select of the same receiver | CID-02 as row 55 | compile error |
| 57 | triple | three `Press{n:1}.button()` | errors on the 2nd and 3rd plus one "first use" error | compile error |
| 58 | not compared | `Press{n:1}` vs `Press{n:2}`; `Press{n}` (shorthand) twice; `Press{n: x}` twice; `Press{n: 1, ..base}`; `route_var.button()` twice; `Press{n:1}` vs `Press{n:1u8}` | compiles (the runtime validator catches real duplicates) | none |

**`components!`: syntax**

| # | Scenario | Input | Expected | Error handling |
|---|---|---|---|---|
| 59 | syntax | unknown keyword with brackets (`box [ ]`), a non-node expression (`5`, `my_var`) at node position, `container(color = x)`, `link(label = "x")` (missing url), `link(url = u, label = "x")` (non-literal url), a missing `;` in a section, `button_with(..)` | a plain syn error naming what was expected (`button_with` arrives in Story 9.3; `link` needs literal `url` and `label`, in that order); no rule id | compile error |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/arity.rs` -- `Arity<N>`, `LegacyRows` and the `legacy_rows!` impl macro. Add `RowButtons`, `SectionTexts` and `GalleryItems` the same way, with doc and `compile_fail` doctests.
- `crates/noctis-ui/src/component/layout.rs` -- `Rgb` (L23: add `TryFrom<u32>` and hidden `pub const fn __from_checked(u32)`), `Section::new` (L89), `ActionRow::buttons` (L135, uses `IntoRowButton`). `RowNode`/`ChildNode`/`TopNode` are private.
- `crates/noctis-ui/src/component/media.rs` -- `MediaGallery::new<const N>` (L211), `Thumbnail::new`/`MediaItem::new(impl Into<Media>)`, `.alt(impl Into<Cow>)`, `Media::url`.
- `crates/noctis-ui/src/component/button.rs`, `select.rs`, `text.rs` -- every text setter takes `impl Into<Cow<'static, str>>`. These stay unchanged; the newtypes only add `From<Newtype> for Cow`.
- `crates/noctis-ui/src/placement.rs` -- `sealed::{Sealed, IntoRowButton, IntoAccessory, …}`, the `RowItem` (`[CV2-05]`) and `SectionAccessory` (`[CV2-07]`) traits.
- `crates/noctis-ui/src/route.rs` L84-133 -- `RouteComponents::{button, string_select, user_select, role_select, mentionable_select, channel_select}`. The macro calls them in UFCS form.
- `crates/noctis-ui/src/message/mod.rs` L200 -- `LegacyMessage::new(impl Into<Cow>)`. It accepts `Content` via `From`.
- `crates/noctis-ui/src/error.rs` -- `UiError::Limits { violations }` is the error of every `try_*`/`TryFrom`.
- `crates/noctis-ui/src/limits/` -- the runtime validator (read-only). `lower.rs` shows the bottom-up, level-by-level lowering into borrowed descriptors that `components!` reuses as a pattern.
- `crates/noctis-ui/src/lib.rs` L141-158 -- the module list and `pub use`.
- `crates/noctis-ui/tests/{components,validate}.rs` -- these build 0-length and oversized arrays (`components.rs` L215; `validate.rs` L298, L313, L515, L887, L931, L1157, plus the 6-button, 4-text and 11-item cases). They stop compiling under the new bounds; see task 4.
- `crates/noctis-rules/src/component.rs` -- the descriptors (`Component`, `Container`, `Section`, `Accessory`, `Media`, `Separator`, `Spacing`, `Button`, `ButtonStyle`, `Select`, `SelectKind`, `SelectOption`):
  - `validate_components` L610: CV2-03 notes `kind[i]: n`, then CV2-04, then a pre-order walk.
  - `Component::Opaque(n)` stands for a hole.
  - The granular functions are at L1045-1140.
  - `check.rs` has `between`/`at_most`.
- `crates/noctis-rules/src/message.rs` L103 -- `validate_content` (MSG-01, path `content`). `len.rs` -- `discord_len`.
- `crates/noctis-macros/src/{lib.rs,common.rs}`:
  - The entry points use `parse_macro_input!` and `syn::Error::into_compile_error`.
  - `common::{error, combine}` give the error format.
  - `route.rs` shows how the derives call `noctis-rules`.
- `crates/noctis/src/lib.rs`:
  - L272-288: `prelude`. Add `Rgb` and the five macros.
  - L290-295: `__private` (keep it; `ui` is already there).
  - L297-475: `pub mod ui` docs and re-exports.
  - Root: add `pub use noctis_macros::{components, content, label, placeholder, rgb};`.
- `crates/noctis/tests/ui.rs` -- trybuild harness (`tests/ui/**/*.rs`). `tests/ui/cv2/CV2-05_button_outside_row.rs` is a fixture example. `tests/boundary.rs` + `boundary/{cv2,cid,msg}.rs` are the boundary modules (a `Handled` Route pattern is in `boundary/cv2.rs`).
- `xtask/rules-pending.toml` -- remove exactly these 11 `story = "4.4"` entries: CID-02, CV2-03, CV2-04, CV2-06, CV2-08, CV2-09, CV2-10, CV2-11, CV2-13, CV2-17, MSG-01. `xtask/src/rules_catalog/mod.rs` L380-410 holds the evidence rules: fixture `tests/ui/<area>/<ID>_<slug>.rs` whose `.stderr` contains `[ID]`.
- `docs/discord-rules.md` -- How text of CV2-06 L194, CV2-07 L195, CV2-08 L196, CV2-09 L197, CV2-10 L198, CV2-11 L199, CV2-13 L201, CV2-17 L205 and MSG-01 L225. Leave the Enforcement columns unchanged.
- `examples/project-template/.claude/skills/noctis/references/components-v2.md`:
  - §0 L45 ("[planned, Story 4.4]")
  - §1 L105
  - §3 L136-138
  - §5 L168-170
  - §8 L224-227
  - §9a L269-270
  - §10a L323 (`try_string_select` → "[planned]")
  - §14 L481-505 (drop "[planned]" and rewrite)
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- the CV2/MSG rows that mention planned macros. `SKILL.md` L21 holds the API hash.
- `examples/project-template/src/modules/{setup,dashboard,profile}.rs` -- these already use `rgb!` and `Rgb` from the prelude (`const ACCENT: Rgb = rgb!(…)`). `setup.rs` L263 builds a static Container by hand.
- `examples/project-template/AGENTS.md` L38 -- the "pre-checked with" bullet.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-rules/src/component.rs` -- add three functions, each with a doctest: `pub fn validate_row_buttons(count: usize, limits: &Limits) -> Vec<Violation>` (CV2-06, `Between{1, row_buttons}`), `validate_section_texts` (CV2-07, `section_texts`) and `validate_gallery_items` (CV2-10, `gallery_items`). The path is `""` and the actual is `Count(n)`. Have `validate_components` reuse the same internal helper so the bounds cannot drift. Update the module docs. Add unit tests `cv2_06_row_buttons_count`, `cv2_07_section_texts_count` and `cv2_10_gallery_items_count` (Matrix 20).
2. `crates/noctis-ui/src/arity.rs` -- add the three sealed traits per Boundaries, implemented for the legal `Arity<N>`. Each gets a passing doctest and a `compile_fail` doctest per bound side (0 and max+1).
3. `crates/noctis-ui/src/component/{layout.rs,media.rs}`:
   - Bound `ActionRow::buttons` with `where Arity<N>: RowButtons`, `Section::new` with `SectionTexts`, and `MediaGallery::new` with `GalleryItems`. Fix their docs, which currently say "checked by the runtime validator".
   - Add `ActionRow::try_buttons<B: RowItem>(buttons: impl IntoIterator<Item = B>, limits: &Limits) -> Result<Self, UiError>`, `Section::try_new<A: SectionAccessory>(texts: impl IntoIterator<Item = TextDisplay>, accessory: A, limits: &Limits) -> Result<Self, UiError>` and `MediaGallery::try_new(items: impl IntoIterator<Item = MediaItem>, limits: &Limits) -> Result<Self, UiError>`. Each collects, counts with the task-1 function and returns `UiError::Limits` on failure.
   - `Rgb`: `impl TryFrom<u32> for Rgb { type Error = UiError; }` via `validate_accent_color`, plus `#[doc(hidden)] pub const fn __from_checked(value: u32) -> Self`.
4. `crates/noctis-ui/tests/{components.rs,validate.rs}` and any doctest that no longer compiles:
   - Replace the 0-length array cases with `try_*` `Err` assertions (Matrix 17-19).
   - Build the 6-button, 4-text and 11-item validator cases with `try_*` under a relaxed `Limits` (`row_buttons = 6`, `section_texts = 4`, `gallery_items = 11`), then assert the validator errors still match Matrix 21 and the 4.3 rows 11, 12 and 22 (paths and values unchanged).
5. `crates/noctis-ui/src/literal.rs` (new module, re-exported; not `component/text.rs`) -- `Label`, `Placeholder` and `Content`:
   - Each is a newtype over `Cow<'static, str>` deriving `Debug, Clone, PartialEq, Eq, Hash`.
   - Methods: `try_new(text: impl Into<Cow<'static, str>>, limits: &Limits) -> Result<Self, UiError>`, `as_str()`, `AsRef<str>`, `From<Self> for Cow<'static, str>`, `TryFrom<&'static str>` and `TryFrom<String>` (both against `Limits::default()`), and a hidden `pub const fn __from_checked(&'static str) -> Self`.
   - Rules: `Label` CV2-08 via `validate_button_label`; `Placeholder` CV2-09 via `validate_select_placeholder`; `Content` MSG-01 via `message::validate_content`.
   - The docs say they never truncate (Lenient truncation is `validate`'s). Doctests show `.label(Label::try_from(..)?)`.
6. `crates/noctis-ui/src/lib.rs` -- `pub use` the arity traits, `Label`, `Placeholder` and `Content`. Add `#[doc(hidden)] pub mod __macro` with `pub fn row_button<B: RowItem>(b: B) -> Button` (via `IntoRowButton`). Crate docs: an "Arity and literals" paragraph.
7. `crates/noctis-macros/Cargo.toml` -- `syn` gets `features = ["full"]`. `crates/noctis-macros/src/lib.rs`:
   - `#[proc_macro] label`, `placeholder`, `content`, `rgb` and `components`. Update the crate doc line.
   - New modules `literal.rs` (the four literal macros, Matrix 1-12) and `components/{mod.rs,parse.rs,lower.rs,expand.rs}` (the layout is the implementer's choice).
   - The literal macros expand to `::noctis::__private::ui::<Type>::__from_checked(lit)` and are usable in `const`.
   - `components!`:
     1. Parse the grammar (Design Notes) into an owned AST that records spans.
     2. Lower it to descriptors bottom-up in local `Vec`s, pattern of `noctis-ui/src/limits/lower.rs`, using the lowering table in Design Notes.
     3. Build a path→span map with the `noctis_rules::component` path scheme.
     4. Run `validate_components(&tree, &Limits::default())`, then the literal checks the descriptors cannot carry (accent integer literal CV2-13, `.button(lit)` label, the options-array count), then CID-02.
     5. On any error, emit every error plus `::noctis::__private::ui::Components::new()`. Otherwise emit the builder expansion.

   Add unit tests in the crate for parsing and lowering (count, CV2-04 span choice, CID-02 key extraction).
8. `crates/noctis/src/lib.rs`:
   - Root `pub use noctis_macros::{components, content, label, placeholder, rgb};` with doc lines.
   - `prelude` adds `Rgb` and the five macros.
   - `ui` re-exports `Label, Placeholder, Content, RowButtons, SectionTexts, GalleryItems`.
   - New `ui` doc groups "Arity" and "Literal macros and `components!`" with doctests (Matrix 22, 37).
9. `crates/noctis/tests/components_macro.rs` (new) -- Matrix 1, 3, 5, 8 (`LegacyMessage::new(content!(..))`), 9, 22-33 (passing forms), 35, 36, 37, 38, 40-42, 44 and 58, all through the facade only. Test names carry the rule snake token where one applies (`cv2_03_…`, `cid_02_…`).
10. `crates/noctis/tests/boundary/{cv2,cid,msg}.rs` -- compile-time boundary passes, each validated through `noctis::ui::validate`:
    - `cv2`: `components!` with exactly 5 buttons, exactly 40 components, exactly 4000 literal characters; an 80-character `label!`; a 150-character `placeholder!`; `rgb!(0xFFFFFF)`; 10 gallery items; 3 Section texts.
    - `cid`: distinct literal payloads (`cid_02_…`).
    - `msg`: `content!` of exactly 2000 (`msg_01_…`).
11. `crates/noctis/tests/ui/{cv2,cid,msg}/` -- trybuild fixtures, each blessed with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`, and each `.stderr` containing its id:
    - `CV2-03_components_41`
    - `CV2-04_components_4001_chars`
    - `CV2-05_components_container_in_container`
    - `CV2-05_components_thumbnail_top_level`
    - `CV2-05_components_button_outside_row`
    - `CV2-06_six_buttons`
    - `CV2-06_zero_buttons`
    - `CV2-06_components_six_buttons`
    - `CV2-06_components_select_with_button`
    - `CV2-07_four_texts`
    - `CV2-07_components_no_text`
    - `CV2-08_label_81`
    - `CV2-08_components_button_label_81`
    - `CV2-08_components_link_url_513`
    - `CV2-09_placeholder_151`
    - `CV2-09_components_26_options`
    - `CV2-10_eleven_items`
    - `CV2-10_components_empty_gallery`
    - `CV2-10_components_item_alt_1025`
    - `CV2-11_components_thumbnail_alt_1025`
    - `CV2-13_rgb_overflow`
    - `CV2-13_components_accent_overflow`
    - `CV2-17_components_media_url_2049`
    - `CID-02_components_duplicate_route` (both spans in `.stderr`)
    - `MSG-01_content_2001`

    The macro-only Matrix rows 2, 4, 7, 10, 11, 39, 43, 45-53, 55-57 and 59 are covered by these fixtures, or by `compile_fail` doctests in `crates/noctis/src/lib.rs` (rows 4, 11, 59; a doctest needs no id).
12. `xtask/rules-pending.toml` -- remove the 11 entries listed in the Code Map. Run `cargo run --release -p xtask -- rules-catalog`, which must pass.
13. `docs/discord-rules.md` -- update the How text, leaving the Enforcement columns unchanged:
    - CV2-06, CV2-07, CV2-10: arity trait + `try_*`.
    - CV2-08: `label!` / `Label::try_new`, and literal link URLs in `components!`.
    - CV2-09: `placeholder!`.
    - CV2-11: `components!` literal alt.
    - CV2-13: `rgb!` and `Rgb::try_from` as they exist now.
    - CV2-17: `components!` literal media URLs.
    - MSG-01: `content!` / `Content::try_new`.
14. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`. Write the new hash into `SKILL.md` L21.
15. `examples/project-template/.claude/skills/noctis/references/components-v2.md`:
    - Rewrite §14 without "[planned]". It covers:
      - the full grammar;
      - the expansion (typed builders, so placement traits apply to holes and spreads);
      - every check with its rule id and the count rules (hole 1, spread 0, non-literal text 0);
      - CID-02's literal-only scope;
      - the error shape;
      - the runtime validator as the backstop.
    - Add a "Literal macros" subsection naming `label!`, `placeholder!`, `content!`, `rgb!`, `Label`, `Placeholder`, `Content`, their `try_new`/`TryFrom`, and `Rgb::try_from`. State that localized text uses the runtime path.
    - In §3/§5/§8, document the arity bounds (`RowButtons`, `SectionTexts`, `GalleryItems`) and `Section::try_new` / `MediaGallery::try_new` / `ActionRow::try_buttons` (replacing the old planned names `try_from_vec`).
    - §1 `rgb!`; §9a `label!`; §10a `try_string_select [planned]`.
    - Update `limits-cheatsheet.md` rows to match.
16. `examples/project-template/src/modules/setup.rs` L263 -- use `components! { container(accent = rgb!(0x57_F287)) [ text(text) ] }`. `examples/project-template/AGENTS.md` L38 -- add "fixed-shape trees with `noctis::components!` and literals with `label!`/`placeholder!`/`content!`/`rgb!` (checked at `cargo check`)".

**Acceptance Criteria:**
- Given a literal passed through `label!`, `placeholder!`, `content!` or `rgb!`, when `cargo check` runs, then an over-limit literal fails with `[CV2-08]`, `[CV2-09]`, `[MSG-01]` or `[CV2-13]` spanned on the literal, and `Label::try_from`, `Placeholder::try_from`, `Content::try_from` and `Rgb::try_from` reject the same value at runtime with the same violation.
- Given `ActionRow::buttons`, `Section::new` or `MediaGallery::new` with an array length outside 1-5, 1-3 or 1-10, when `cargo check` runs, then it fails with an E0277 whose message starts with `[CV2-06]`, `[CV2-07]` or `[CV2-10]`.
- Given a `components!` literal tree, when it is valid, then it evaluates to a `Components` that `V2Message::new` accepts and whose JSON equals the hand-built tree. When it breaks a static rule, then `cargo check` fails with span-precise errors naming each rule id (CV2-03 with per-subtree notes, CID-02 with both spans).
- Given the UI and boundary suites, when `cargo test --release -p noctis` runs, then every fixture listed in task 11 matches its blessed `.stderr`, and the boundary passes compile and validate.
- Given the catalog, when `cargo run --release -p xtask -- rules-catalog` runs, then it passes with no `story = "4.4"` entry left in `rules-pending.toml`.
- Given the new public API, when `api-snapshot` and `skill-check` run, then both pass and `components-v2.md` documents `components!` (grammar, checks, holes and spreads), `label!`, `placeholder!` and `rgb!`.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 30 findings — high 0, medium 2, low 12, false 15, maybe-false 1
- findings:
  - `[low]` `[reject]` rgb!/accent literal suffix ignored (`rgb!(0x1FFu8)` retyped to u32) — real (`base10_parse::<u32>` drops the suffix, literal.rs:103, expand.rs:111), but the value is still a valid colour, non-u32 suffixes are unlikely, and the fix adds a suffix guard.
  - `[low]` `[patch]` select_counts keeps the last literal min/max_values even when a later non-literal call overrides it (false CV2-09) — confirmed at lower.rs:386-406; shares root cause with edge #1; fix: a non-literal call of the same setter drops the earlier literal.
  - `[low]` `[patch]` a pre-built select cannot be a `{hole}` inside `row [..]` (wrapped in `row_button`, E0277 `[CV2-05] ... Action Row button`) — confirmed expand.rs:145-153; grammar says `(button | {hole})` without stating it; fix: document that a row hole is a button and a dynamic select goes in as `{ActionRow::select(sel)}`.
  - `[low]` `[patch]` peek_keyword takes `row[0].button()` / `text(x).button()` as a node keyword — confirmed parse.rs:305-318 (keyword + paren/bracket, no check for a following `.`); shares root cause with edge #3; fix: after the group, a following `.` means a Route expression.
  - `[maybe-false]` `[reject]` error path drops user expressions, causing unused-variable warnings — would need a fixture with a tree-only variable to see whether rustc emits liveness lints after the compile_error!s; if true only cosmetic (low).
  - `[low]` `[reject]` CV2-03 emits one note per top-level node (wall of notes) — Matrix 39 mandates exactly this; the fix would edit the spec.
  - `[low]` `[reject]` `link(..)` takes no setters and needs literal url/label — Matrix 59 and the grammar line settle the form; accepting setters adds grammar surface, and `{hole}` is the documented way out.
  - `[low]` `[reject]` newtypes lack `TryFrom<&str>`/`Display` — signatures are fixed by task 5; the fix would edit the spec.
  - `[false]` `[reject]` CID-02 "first use" error lacks `= help:` — the help convention applies to a rule violation's message; the first-use error is the secondary span marker whose text Matrix 55 fixes.
  - `[low]` `[patch]` misuse paths pinned only by id-less doctests; no `components!` tests for CV2-09 min>max / placeholder 151 / misplaced `item` — doctests are allowed by task 11, but Matrix 49 placeholder/min>max and 45 `item` have no test; grouped with vgap #1; fix: add the cases.
  - `[false]` `[reject]` CID-02 on two identical select receivers is a false positive because of the nonce — the select nonce is per render (routes-payloads-refs.md L69, L323), so both selects carry the same id; Matrix 56 requires the button/select duplicate.
  - `[low]` `[patch]` grammar shows `spacing` as required in `separator(..)`, and literal vs optional brackets are unquoted in the rustdoc grammar — confirmed components-v2.md:533 vs parse accepting `separator(divider = false)`; fix: make spacing optional in both grammars and quote literal brackets in `crates/noctis/src/lib.rs`.
  - `[false]` `[reject]` `string_select(expr)` lowered as User weakens CV2-09 — the Design Notes lowering table settles it, user-select bounds are only looser (no false error), and the docs already say "array-literal options".
  - `[low]` `[patch]` (edge) select_counts override — same as blind #2.
  - `[low]` `[patch]` (edge) CID-02 rejects two equal literal values of a `by_ref` Route although runtime Refs are minted randomly (`Ref::mint`, noctis-codec refs.rs:53) — real but rare; the macro cannot see `by_ref`; fix: state the limit in the skill's CID-02 section (use `{hole}` or distinct values).
  - `[low]` `[patch]` (edge) keyword-named receiver misparse — same as blind #4.
  - `[false]` `[reject]` (edge) `separator(spacing = Spacing::Large)` reports CV2-13 — Matrix 26 mandates CE[CV2-13] for non-`small`/`large` spacing, and CV2-13 covers Separator spacing (discord-rules.md L201).
  - `[medium]` `[patch]` (vgap) no `components!` test for an over-limit literal `.placeholder` (Matrix 49) — pre-verified; fix: add a lower.rs `rules_and_own_checks` case (or fixture) asserting `[CV2-09] row[0].select[0].placeholder`.
  - `[medium]` `[patch]` (vgap) CID-02 untested for accessory + container row, unit/tuple receivers, button + select (Matrix 56) — pre-verified; fix: extend `cid_02_duplicates`.
  - `[false]` `[reject]` (intent) builder setters still take `impl Into<Cow>` — Design Notes ("Why newtypes convert into Cow") and the Code Map settle it.
  - `[false]` `[reject]` (intent) no attribute-macro form — the spec reads "builder macro attribute" as the `components!` literal positions.
  - `[false]` `[reject]` (intent) no standalone description newtype for CV2-11 — Never excludes `alt!`/`lit!`; Matrix 50-51 place CV2-10/11 alt checks in `components!`.
  - `[false]` `[reject]` (intent) `button_with` rejected — Never list and Matrix 59 (Story 9.3).
  - `[false]` `[reject]` (intent) `View::render` untested — Never list (Epic 8).
  - `[false]` `[reject]` (intent) CID-02 as two errors — Matrix 55 specifies the two errors.
  - `[false]` `[reject]` (intent) CID-02 key heuristics — Design Notes "CID-02 key" and Matrix 56/58 settle them.
  - `[false]` `[reject]` (intent) grammar extensions — Design Notes grammar resolutions and Matrix 25-31.
  - `[false]` `[reject]` (intent) MSG-01 / CV2-17 beyond the epic Implements — the intent Problem names both rules.
  - `[false]` `[reject]` (intent) CV2-03 notes per top-level node only — Matrix 39.
  - `[false]` `[reject]` (intent) no CV2-15 work — already enforced by the pre-existing `LegacyRows` bound; nothing was left to do.

## Design Notes

- **Why newtypes convert into `Cow`:** the AC wants builders to take `Label`/`Placeholder`, and the template passes localized `String`s to `.label(..)`. If every text setter keeps `impl Into<Cow<'static, str>>` and `From<Label> for Cow` exists, both work and nothing breaks. `TryFrom<String>` cannot coexist with an unchecked `From<String>`, so there is no `From<String> for Label`. `try_*` and `TryFrom` never truncate; Lenient truncation stays in `validate`.
- **Why `try_*` take `&Limits`:** AD-3 says every measuring function takes `&Limits`. `TryFrom` (which cannot take arguments) uses `Limits::default()`. The arity traits are fixed at Discord's defaults, and a lowered `Limits` is enforced by `validate` (Matrix 21).
- **Grammar** (the story's, with these resolutions):
  - `section` and `gallery` accept zero items, so the count is diagnosed (CV2-07/CV2-10) instead of failing to parse.
  - A `row` accepts buttons, selects and `{hole}`s.
  - `accessory = {hole}` is allowed.
  - `.button()` takes zero arguments or one (the label).
  - `button_with` is rejected until Story 9.3.
  - Any expression at node position whose method chain contains `.button(..)` or `.<kind>_select(..)` is a Route component. The receiver of that call is the Route value; later calls are setters, re-applied in order.
- **Lowering table** (macro → descriptor):

  | Macro form | Descriptor |
  | --- | --- |
  | `text(lit)` | `Text(lit)` |
  | `text(expr)` | `Text("")` |
  | hole | `Opaque(1)`; as an accessory, `Accessory::Button(Button::new(Secondary))` |
  | spread | omitted |
  | non-literal media | `Media::new("https://x")`, a valid sentinel |
  | literal media | `Media::new(lit)` |
  | `file(expr)` | `File("attachment://x")` |
  | Route button | `Button::new(Secondary)`, with `.label(lit)` when literal |
  | `link` | `Button::new(Link(url)).label(label)` |
  | `premium` | `Button::new(Premium)` |
  | `string_select([e; n])` | `SelectKind::String` with n placeholder options `("x","x")` |
  | `string_select(expr)` | `SelectKind::User`, so options are not counted |

  The descriptor gets `placeholder`, `min_values` and `max_values` when they are literals. An accent integer literal is checked with `validate_accent_color`, and the descriptor `accent_color` is not set. Any other `accent` expression is not checked.
- **Spans:**
  - Record a span for every node path and for the literal field paths `.label`, `.url`, `.placeholder`, `.description`, `.min_values`, `.max_values` and `.options`.
  - A violation takes its exact path, else the longest recorded prefix, else `Span::call_site()`.
  - The `components` path takes the node that crosses the limit (CV2-03, pre-order running count) or the literal that crosses it (CV2-04, running `discord_len`).
- **CID-02 key:** a receiver qualifies when it is one of these:
  - a struct literal without `..rest` whose every field value is a literal or a negated literal;
  - a call whose callee path's last segment starts with an ASCII uppercase letter and whose arguments are all literals;
  - a path whose last segment starts with an uppercase letter, and has no generics.

  The key is `receiver.to_token_stream().to_string()`. Everything else is left to the runtime validator.
- **Golden tree `T1`** (Matrix 22):
  ```rust
  components! {
      container(accent = rgb!(0x5865_F2)) [
          text("### Settings"),
          separator(spacing = large),
          section [ text("Logs"), text(name); accessory = Press { n: 1 }.button("Change") ],
          gallery [ item("https://x.y/a.png", alt = "A") ],
          row [ Press { n: 2 }.button("Save").style(ButtonStyle::Primary), link(url = "https://x.y", label = "Docs") ],
          row [ Pick.user_select().placeholder("Who").max_values(3) ],
          ..extra,
      ],
      {footer},
  }
  ```
  It equals `Components::new().push(Container::new().accent(Rgb::new(0x58,0x65,0xF2)).push(…)…).push(footer)` built by hand.
- Syntax errors carry no rule id, because they are not Discord rules (the derives' declaration-error convention, CID-03, covers Route declarations only). The literal macros' misuse errors cite their rule, because the macro exists for that rule.

## Verification

**Commands** (repo root; CI is paused, so local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (UI fixtures, boundary, components_macro, noctis-ui, noctis-rules, macros unit tests, doctests).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- exit 0 each.
- `bash .github/scripts/check-build-config.sh` -- exit 0; `test ! -d target/debug` -- true.
- `grep -c 'story = "4.4"' xtask/rules-pending.toml` -- prints 0.

## Auto Run Result

- **Summary:** compile-time Discord limit checks: checked text newtypes (`Label`, `Placeholder`, ...) with `try_*`/`TryFrom` and literal macros, arity traits, `rgb!`, and the `components!` tree macro (noctis-macros `components/{parse,lower,expand}`) that lowers a literal tree to descriptors and runs the runtime rules at compile time (CV2-03/04/05/07/09/10/11/13/17, MSG-01, CID-02) with path-precise spans.
- **Files changed:** `crates/noctis-macros/src/components/*` (parser, lowering, expansion); `crates/noctis/src/lib.rs` (macro re-export, rustdoc grammar); newtype/arity modules in noctis core crates; `crates/noctis/tests/ui/**` (compile_fail fixtures per rule id); `docs/discord-rules.md`, `xtask/rules-pending.toml` (rules moved to enforced); `xtask/public-api.txt` (re-blessed); `examples/project-template/` (setup.rs, AGENTS.md, noctis skill: components-v2, limits-cheatsheet, responder, API hash).
- **Review findings:** 30 total (high 0, medium 2, low 12, false 15, maybe-false 1). Patched: 2 medium (placeholder-151 / min>max / misplaced `item` tests; CID-02 coverage for accessory, container row, unit/tuple receivers, button+select) and 6 low (select_counts non-literal override, row-hole doc, keyword-receiver misparse `row[0].button()`, separator `spacing` optional + quoted grammar brackets, `by_ref` CID-02 limit documented, duplicate edge entries). Deferred: none. Rejected: rgb! literal suffix ignored (still valid colour), unused-variable warnings on error path (unverified, cosmetic), CV2-03 per-node notes (Matrix 39), `link(..)` setters (Matrix 59), newtype `TryFrom<&str>`/`Display` (task 5 signatures), plus 15 false findings settled by spec Design Notes / Matrix / Never list.
- **Follow-up review recommended: true** (first pass, 2 medium patched; high 0, medium 2, low 6). Named risk: the parser change for keyword-named receivers (`.`-after-group lookahead) and the select_counts override logic were patched after review and have not been independently re-reviewed for new misparses of valid trees.
- **Verification:** all local gates green via gates.sh (fmt, clippy -D warnings, test --release, cargo hack --each-feature, deps-check, rules-catalog, api-snapshot, skill-check, cargo deny, MSRV 1.91). CI paused by maintainer.
- **Residual risks:** rustc diagnostic text in trybuild `.stderr` may drift with toolchain updates; CID-02 cannot see `by_ref` Routes (documented); non-literal `string_select(expr)` options are only checked at runtime.
