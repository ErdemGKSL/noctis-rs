---
title: 'Story 4.5: Modal component model and modal validation'
type: 'feature'
created: '2026-10-20'
status: 'in-progress'
baseline_revision: '5911db8d264ce7b04169a5f7a877246171ec7634'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/modals.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Noctis has modal rule functions (`noctis_rules::modal`, Story 1.6) but no typed modal model. A bot author cannot build a modal with Label, Text Input, modal selects, File Upload, Radio Group, Checkbox Group or Checkbox. Nothing stops message components in a modal or `disabled` on a modal select. The runtime validator does not accept modals (FR-29, FR-63, FR-78; AD-12; MOD-01..MOD-08). `xtask/rules-pending.toml` still holds the `MOD-03` diagnosed entry for this story.

**Approach:**
- Add the modal model to `noctis-ui`. It has one type per modal component, and a `Modal` built only from a Route value (`RouteComponents::modal`).
- Placement uses two sealed traits whose diagnostics cite `[MOD-03]`: `ModalTopLevel` and `ModalInput`.
- The types serialize straight to Discord's modal callback `data` JSON.
- Extend the sealed `Validate` to `Modal`. `validate`/`cost` lower a modal into `noctis_rules::modal` descriptors, call `validate_modal`, then check the input `custom_id`s (CID-01, CID-02).
- Document every modal component in the Usage Skill.

## Boundaries & Constraints

**Always:**
- **Names.** All new types are flat in `noctis_ui` and re-exported from `noctis::ui`:
  - `Modal` and `ModalComponent`;
  - `ModalLabel`, which is Discord's Label component. `noctis::ui::Label` stays the Story 4.4 CV2-08 literal newtype and is unchanged;
  - `TextInput` and `TextInputStyle` (`pub use noctis_rules::modal::TextInputStyle`);
  - `ModalStringSelect`, `ModalUserSelect`, `ModalRoleSelect`, `ModalMentionableSelect`, `ModalChannelSelect`;
  - `FileUpload`, `ModalOption`, `RadioGroup`, `CheckboxGroup`, `Checkbox`;
  - the traits `ModalTopLevel` and `ModalInput`.
- **Builder style.** Builders follow the 4.1 style: by-value `#[must_use]` setters, text as `impl Into<Cow<'static, str>>`, `Debug` + `Clone`. Fields are private (`pub(crate)`).
- **Integer types.** Lengths are `u16`, value counts are `u8`, matching the message selects. Every constructor accepts any count; only the validator rejects counts.
- **The `Modal` constructor.** The only one is `RouteComponents::modal(self, title: impl Into<Cow<'static, str>>) -> Modal` with `where Self: Route<Payload = Self> + Handled + Send + Sync + 'static`. Its `custom_id` is a `CustomId::new(self)`; this covers MOD-08, and the bound gives CID-04.
  - A modal has no `Modal::new`, no string id and no `Default`.
  - It offers `push<C: ModalTopLevel>(self, c) -> Self`, `title(&self) -> &str`, `len`, `is_empty`, and hidden `custom_id_mut(&mut self) -> &mut CustomId` for core's AD-18 resolution.
  - Serializing a modal with an unresolved id is a serde error, like messages.
- **Inputs.**
  - Each input constructor takes `key: impl Into<Cow<'static, str>>` first. The key is serialized as the input's `custom_id` and names the submission field (Story 4.6). It is never a Route.
  - `ModalLabel::new(label, input: impl ModalInput)` plus `.description(..)`.
  - `TextInput::new(key, style)` plus `.min_length(u16)`, `.max_length(u16)`, `.required(bool)`, `.value(..)` (pre-fill, FR-63) and `.placeholder(..)`.
  - `ModalStringSelect::new(key, options: impl IntoIterator<Item = SelectOption>)`, plus `ModalUserSelect::new(key)`, `ModalRoleSelect::new(key)`, `ModalMentionableSelect::new(key)` and `ModalChannelSelect::new(key)`. All five take `.placeholder`, `.min_values(u8)`, `.max_values(u8)` and `.required(bool)`. The auto-populated ones also take `default_users` / `default_roles` / `default_channels` as their message twins do, and Channel takes `channel_types`.
  - **No modal select has `disabled`.**
  - `FileUpload::new(key)` plus `.min_values(u8)`, `.max_values(u8)`, `.required(bool)` and `.file_types(impl IntoIterator<Item = impl Into<Cow<'static, str>>>)`, which appends.
  - `ModalOption::new(label, value)` plus `.description(..)` and `.default(bool)`.
  - `RadioGroup::new(key, options: impl IntoIterator<Item = ModalOption>)` plus `.required(bool)`.
  - `CheckboxGroup::new(key, options)` plus `.min_values(u8)`, `.max_values(u8)` and `.required(bool)`.
  - `Checkbox::new(key)` plus `.default(bool)`. **Checkbox has no `required`.**
- **Placement traits.** Both are sealed, with `#[diagnostic::on_unimplemented]` messages starting `[MOD-03]`:
  - `ModalTopLevel` is implemented by `ModalLabel`, `TextDisplay` and `ModalComponent`.
  - `ModalInput` is implemented by the ten Label inputs.
  - Add their private conversions to `placement::sealed` beside the existing ones.
  - `ModalComponent` is opaque, with `From<ModalLabel>` and `From<TextDisplay>`.
- **JSON** (Discord field order, unset fields omitted, no numeric `id` anywhere in a modal):
  - Modal: `{"custom_id","title","components":[…]}`.
  - Label: `{"type":18,"label","description"?,"component":{…}}`.
  - Text Input: `{"type":4,"custom_id","style","min_length"?,"max_length"?,"required":false?,"value"?,"placeholder"?}`. **Never a `label` key; never wrapped in an Action Row (type 1).**
  - Modal selects: as the message selects (types 3/5/6/7/8) with `"required":false` written only when false, and never `disabled`.
  - File Upload: `{"type":19,"custom_id","min_values"?,"max_values"?,"required":false?,"file_types"?}`; `file_types` only when non-empty.
  - Radio Group: `{"type":21,"custom_id","options":[…],"required":false?}`.
  - Checkbox Group: `{"type":22,"custom_id","options","min_values"?,"max_values"?,"required":false?}`.
  - Checkbox: `{"type":23,"custom_id","default":true?}`.
  - ModalOption: `{"value","label","description"?,"default":true?}`.
  - Text Display: `{"type":10,"content"}`.
- **Validation** (`validate(&mut modal, &limits)` / `cost(&modal, &limits)`):
  1. In Lenient mode only, truncate first (via the existing `lenient` helpers and `Limits::truncates`, each a `LimitWarning::Truncated`). The truncated fields are: title (MOD-01), Label text and description (MOD-04), Text Input placeholder (MOD-05), modal select placeholder (CV2-09), String Select option label and description (CV2-09), and Radio/Checkbox option label and description (MOD-07). **Never** truncate Text Input `value`, option values, keys or counts.
  2. Lower into `noctis_rules::modal` descriptors. `TextDisplay` becomes `ModalComponent::Text`. A modal select becomes `ModalSelect::new(component::Select{kind, placeholder, min/max, default_values count, disabled=false}).required(..)`.
  3. Run `validate_modal` and keep its violations in its order.
  4. Then run CID-01 on every input key in pre-order, via the new `noctis_rules::modal::validate_input_custom_id`, with the path re-rooted to `<rules child path>.custom_id`.
  5. Then CID-02 duplicate keys: each later use is a violation at its `….custom_id` path, with the note `first used at <path>`. The pattern is `limits::check_custom_ids`.
  6. Errors are `UiError::Limits { violations }`. Warnings are logged through the existing `finish`.
  7. `CostReport.components` = top-level count + one per Label input. `text_chars` = the `discord_len` sum of the Text Displays. Warnings: truncations only.
- Every new public item has rustdoc with a doctest. Run only release builds/tests (AGENTS.md); trybuild needs no `--release`.

**Never:**
- No `disabled` method on any modal type. No `required` on `Checkbox`. No Action Row in modals. No string `custom_id` for `Modal`.
- No `derive(Modal)`, submission decoding, `derive(Choices)`, compile-time option-count arity or download helper. These are Story 4.6.
- No responder or callback type-9 wrapper. That is core's (Story 7.8).
- Do not change `noctis_rules::modal::validate_modal` semantics or paths.
- Do not rename the 4.4 `Label`/`Placeholder`.
- No new crate dependency.
- No fixture `.stderr` or test name may create evidence for a rule still pending for 4.6:
  - no `tests/ui/mod/MOD-0{1,2,4,5,6,7}_*` fixture;
  - no test fn named with `mod_09`.

## I/O & Edge-Case Matrix

The default Limits apply. "err R @ P" means a `UiError::Limits` that contains rule R at path P. Paths follow `noctis_rules::modal`.

| # | Scenario | Input | Expected |
|---|---|---|---|
| 1 | Happy full modal | `Save.modal("Report")` + Text Display + 4 Labels (TextInput, ModalStringSelect, RadioGroup 2 opts, Checkbox) | Ok; components 9; JSON = insta snapshot; no `"type":1`, no `label` key in type 4 |
| 2 | Top-level 5 | 5 Labels | Ok |
| 3 | Top-level 6 | 6 Labels | err MOD-02 @ `components` (Between 1..5, actual 6) |
| 4 | Empty modal | no push | err MOD-02 @ `components` (actual 0) |
| 5 | Title 45 / 46 / empty | `"a"×45`, `"a"×46`, `""` | Ok / err MOD-01 @ `title` / err MOD-01 @ `title` |
| 6 | Title unicode | `"é"×45`, `"😀"×23` (UTF-16 46) | Ok / err MOD-01 (discord_len 46) |
| 7 | Label text 45 / 46 / empty | Label text | Ok / err MOD-04 @ `label[0].label` / err MOD-04 |
| 8 | Label description 100 / 101 / empty | `.description` | Ok / err MOD-04 @ `label[0].description` / Ok |
| 9 | Text Input min_length 0 / 4000 / 4001 | `.min_length` | Ok / Ok / err MOD-05 @ `label[0].text_input.min_length` |
| 10 | Text Input max_length 0 / 1 / 4000 / 4001 | `.max_length` | err / Ok / Ok / err MOD-05 @ `….max_length` |
| 11 | Text Input min > max | min 10 max 5 | err MOD-05 @ `label[0].text_input` |
| 12 | Pre-fill value 4000 / 4001 | `.value` | Ok / err MOD-05 @ `….value` (also in Lenient: not truncated) |
| 13 | Text Input placeholder 100 / 101 | `.placeholder` | Ok / err MOD-05 @ `….placeholder` |
| 14 | File Upload min 0 + required | `.min_values(0)` (required default) | err MOD-06 @ `label[0].file_upload.min_values` |
| 15 | File Upload min 0 not required; max 10 / 11; max 0 | `.required(false).min_values(0)`; `.max_values(10/11/0)` | Ok; Ok / err MOD-06 @ `….max_values`; err |
| 16 | File Upload min 3 max 2 | | err MOD-06 @ `label[0].file_upload` |
| 17 | file_types 10 / 11 / bad entry | `["image"…]`, 11 entries, `"exe"` | Ok / err MOD-06 @ `….file_types` / err MOD-06 @ `….file_types[0]` |
| 18 | Radio options 1 / 2 / 10 / 11 / 0 | | err MOD-07 @ `label[0].radio_group.options` / Ok / Ok / err / err |
| 19 | Checkbox Group options 0 / 1 / 10 / 11 | | err / Ok / Ok / err MOD-07 @ `….checkbox_group.options` |
| 20 | Checkbox Group min 0 + required | `.min_values(0)` | err MOD-07 @ `….checkbox_group.min_values` |
| 21 | Checkbox Group min 0 not required; max 11; min 4 > options 3 (max default) | | Ok; err MOD-07 @ `….max_values`; err MOD-07 @ `label[0].checkbox_group` |
| 22 | Option label/value 100 / 101 / empty; description 101 | ModalOption | Ok / err MOD-07 @ `….options[k].label` (value: `.value`) / err / err `.description` |
| 23 | Modal select min 0 + required | `ModalUserSelect::new("u").min_values(0)` | err CV2-09 @ `label[0].select.min_values` |
| 24 | Modal String Select 0 / 25 / 26 options; placeholder 150 / 151 | | err CV2-09 / Ok / err / Ok / err @ `label[0].select.placeholder` |
| 25 | Key length 1 / 100 / 101 / empty / unicode | `TextInput::new(k, …)` | Ok / Ok / err CID-01 @ `label[0].text_input.custom_id` / err CID-01 / measured by discord_len |
| 26 | Duplicate keys | two Labels with key `"a"` | err CID-02 @ `label[1].<kind>.custom_id`, note `first used at label[0].<kind>.custom_id` |
| 27 | Checkbox | `Checkbox::new("c").default(true)` | Ok; JSON `{"type":23,"custom_id":"c","default":true}` |
| 28 | Lenient title 50, Label 50, option label 101 | `LimitMode::Lenient` | Ok; 3 `Truncated` warnings (MOD-01, MOD-04, MOD-07) in pre-order; texts end with `…` and fit |
| 29 | Lenient + empty title / 6 Labels / bad key | | still err (structural, min, count, CID) |
| 30 | Violation order | title 46 + 6 comps + dup key | MOD-01, MOD-02, …rules order…, CID-01s, CID-02s |
| 31 | Unresolved modal id | Route declared `view` (or `select`) | `serde_json::to_value` errors naming the route; `custom_id_mut().resolve(..)` then serializes |
| 32 | cost | modal of #1 | `{components: 9, text_chars: len(text)}`, no warnings, modal unchanged |
| 33 | Strict never mutates | over-long title, Strict | err; title unchanged |
| 34 | `required` false emitted | `TextInput.required(false)`, `RadioGroup.required(false)` | `"required":false`; default omits the key |
| 35 | Compile: message comp at top | `.push(LinkButton::new(..))` | E0277 `[MOD-03] … cannot be a top-level modal component` |
| 36 | Compile: message comp in Label | `ModalLabel::new("L", Container::new())`, message `StringSelect` | E0277 `[MOD-03] … cannot be a modal Label input` |
| 37 | Compile: `.disabled(true)` on a modal select; `Checkbox.required(true)`; `Modal::new` | | E0599 (compile_fail doctests) |
| 38 | Compile: Route without handler `.modal(..)` | | E0277 `[CID-04]` (doctest) |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/route.rs` -- `RouteComponents` (L74-133): add `fn modal`. `CustomId::new` / `resolve` / `Serialize` are reused unchanged.
- `crates/noctis-ui/src/component/select.rs` -- `SelectOption` (reuse in `ModalStringSelect`), `DefaultValue` (private; make `pub(crate)`), `wire_select` and `common_setters!`. Refactor so the modal selects share the option/default serialization. `Common.custom_id` is a `CustomId`, while a modal key is a `Cow`, so pass the id as `&dyn erased`/generic `impl Serialize` or add a modal-specific `wire_modal_select`. This is the implementer's choice; the JSON order is the same either way.
- `crates/noctis-ui/src/component/text.rs` -- `TextDisplay` (`content` is pub(crate)), reused at the modal top level.
- `crates/noctis-ui/src/wire.rs` -- `Wire`, `head`, `opt`, `flag` and `serialize_via_wire!`. Modals pass `ids = None`.
- `crates/noctis-ui/src/placement.rs` -- the `sealed` module and the `sealed!`/impl macros. Add `IntoModalTop`/`IntoModalInput` and the two public traits.
- `crates/noctis-ui/src/limits/mod.rs` -- `Validate` + `sealed::Check` impls (L37-50, L203-275), `check_custom_ids` (L295) as the CID-02 pattern, and `finish`. `limits/lenient.rs` -- `fit(rule, max, path, &mut Cow, limits)` (truncation helper). `limits/lower.rs` L233-266 -- `lower_select`/`lower_option`, the pattern for descriptors that borrow from level Vecs.
- `crates/noctis-ui/src/error.rs` -- `UiError::Limits`, `checked`.
- `crates/noctis-ui/src/lib.rs` -- module list L141-152, `pub use` L154-171, and crate docs (add a "# Modals" section).
- `crates/noctis-rules/src/modal.rs` -- the descriptors and `validate_modal` (read-only except the new function). `check.rs` `text_between`. `limits.rs` `custom_id_len` (CID-01).
- `crates/noctis/src/lib.rs` -- `pub mod ui` docs (L318-603) and the re-export list (L604-616).
- `crates/noctis/tests/ui.rs` -- the trybuild glob `tests/ui/**/*.rs`. Fixtures are named `tests/ui/<area>/<ID>_<slug>.rs`; for MOD-03 the area dir is `mod`. `tests/boundary.rs` + `boundary/cv2.rs` show the `#[path]` module and the Handled Route pattern (`impl ::noctis::__private::ui::Handled for X {}`).
- `crates/noctis-ui/tests/components.rs` -- insta snapshot style (`insta::assert_json_snapshot!`).
- `xtask/rules-pending.toml` L296-299 -- the `MOD-03` entry to remove. `xtask/src/rules_catalog/mod.rs` -- the fixture evidence rules. Compile_fail doctest tags `// [ID]` count as by-construction and are harmless for diagnosed-only rules.
- `xtask/src/skill_check/components.rs` L26-49 `BUILDERS` (+ unit tests L190+).
- `docs/discord-rules.md` L211-218 -- the How text of MOD-01..MOD-08; CID-01 L174.
- `examples/project-template/.claude/skills/noctis/references/modals.md` §4 (component entries), §7 (rule functions); `components-v2.md` §13 table L493; `limits-cheatsheet.md` MOD rows; `SKILL.md` L21 API hash; `examples/project-template/AGENTS.md` L38.
- Continuity (Story 4.4): `Label`/`Placeholder` literal newtypes, arity traits and `components!` stay unchanged. The verification gates and trybuild bless command are the same as 4.4.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-rules/src/modal.rs` -- add `pub fn validate_input_custom_id(id: &str, limits: &Limits) -> Vec<Violation>`:
   - CID-01, `text_between(1, limits.custom_id_len)`, path `""`.
   - Give it a doctest. Update the module docs (CID-01 for input keys).
   - Unit tests `cid_01_modal_input_custom_id_bounds` (1, 100, 101, empty, 50 emoji = UTF-16 100 passes, 51 fails) in `modal/tests.rs`.
2. `crates/noctis-ui/src/modal/` (new module: `mod.rs` for `Modal`/`ModalComponent`/`ModalLabel`, `input.rs` for TextInput/FileUpload/Radio/CheckboxGroup/Checkbox/ModalOption, `select.rs` for the modal selects) -- the types, setters, `Wire` impls and `Serialize` per Boundaries. Each component's rustdoc gives its type number, placement and a JSON doctest. Add compile_fail doctests:
   - `ModalUserSelect::new("u").disabled(true); // [MOD-03]` (E0599);
   - `Checkbox::new("c").required(true); // [MOD-07]` (E0599);
   - `Modal::new("id", "t")` (E0599);
   - `.modal("t")` on a Route without `Handled` (E0277).
3. `crates/noctis-ui/src/placement.rs` -- `ModalTopLevel` (message `[MOD-03] \`{Self}\` cannot be a top-level modal component`, notes: "a modal's top level takes a ModalLabel or a TextDisplay (MOD-02: 1 to 5)" and "buttons, Sections, Containers, galleries, files and Action Rows never go in a modal") and `ModalInput` (`[MOD-03] \`{Self}\` cannot be a modal Label input`, a note listing the ten inputs, and a note "message selects are not modal selects: use ModalStringSelect, ModalUserSelect, …"). Implement both as in Boundaries.
4. `crates/noctis-ui/src/route.rs` -- `RouteComponents::modal` with a doctest.
5. `crates/noctis-ui/src/limits/` -- `impl Validate for Modal` (+ `Sealed`, `Check`):
   - Lenient fitting goes in `lenient.rs` (`fit_modal`).
   - Lowering goes in `lower.rs` (`with_modal`).
   - Key checks follow the Validation order.
   - Update the `validate`/`cost`/`Validate` docs to name `Modal`.
6. `crates/noctis-ui/src/lib.rs` -- `mod modal;`, the `pub use` of every new item, the hidden `CustomId` unchanged, and a "# Modals" crate-doc section with a doctest.
7. `crates/noctis-ui/tests/modal.rs` (new) -- one test per Matrix row 1-34. Names cite the rule snake id (`mod_02_six_top_level`, `mod_07_radio_one_option`, `cid_02_duplicate_keys`, …; never `mod_09`). Row 1 gets an insta JSON snapshot (`snapshots/modal__full.snap`) plus explicit asserts that no object has `"type":1` and that the type-4 object has no `label`. The test file needs a hand-written Route with `Handled`, as the doctests in `route.rs` show.
8. `crates/noctis/src/lib.rs` -- re-export the new items in `ui`. Add a "Modals" doc group with a doctest built through the facade (`noctis::prelude::*` and `#[derive(Route)]`).
9. `crates/noctis/tests/ui/mod/` -- trybuild fixtures, blessed with `TRYBUILD=overwrite cargo test --release -p noctis --test ui`. Each `.stderr` must contain `[MOD-03]`:
   - `MOD-03_button_in_modal.rs` (Matrix 35);
   - `MOD-03_container_in_label.rs`;
   - `MOD-03_message_select_in_label.rs` (a `select` Route with `Handled`) (Matrix 36).
10. `crates/noctis/tests/boundary.rs` + `boundary/modal.rs` (new) -- boundary passes through `noctis::ui::validate`:
    - 5 top-level Labels;
    - 45-char title and Label;
    - 100-char description;
    - Text Input min 0 / max 4000 / value 4000 / placeholder 100;
    - File Upload max 10 with 10 file types;
    - Radio 2 and 10 options;
    - Checkbox Group 1 and 10;
    - 100-char option label/value;
    - 100-char key.

    Names are `mod_0x_…` / `cid_01_…`.
11. `xtask/rules-pending.toml` -- remove the `MOD-03` entry. `xtask/src/skill_check/components.rs` -- add `ModalLabel`, `ModalStringSelect`, `ModalUserSelect`, `ModalRoleSelect`, `ModalMentionableSelect` and `ModalChannelSelect` to `BUILDERS`, and fix any unit test that enumerates the list.
12. `docs/discord-rules.md` -- leave the Enforcement columns unchanged and edit only the How text:
    - MOD-03: `ModalTopLevel` / `ModalInput` (`[MOD-03]`), modal selects without `disabled`.
    - MOD-07: `Checkbox` has no `required`; runtime counts via `noctis::ui::validate`.
    - MOD-08: `RouteComponents::modal`.
    - CID-01: modal input keys checked at runtime by `validate_input_custom_id`.
13. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`. Put the printed hash in `SKILL.md` L21.
14. `examples/project-template/.claude/skills/noctis/references/modals.md`:
    - Add "## 4b. Building a modal at runtime: `Modal`". It covers `RouteComponents::modal`, `push`, keys, JSON shape, `validate`/`cost` on modals, the Lenient fields and the error paths.
    - Turn each §4 component heading into an entry naming exactly one builder in backticks, with `**Where:**`, `**Limits:**` and `**Builder:**`. The entries are:
      - `ModalLabel`, `TextInput`, `FileUpload`, `RadioGroup`, `CheckboxGroup`, `Checkbox`;
      - one entry per modal select type: `ModalStringSelect`, `ModalUserSelect`, `ModalRoleSelect`, `ModalMentionableSelect`, `ModalChannelSelect`.
    - Never put `TextDisplay` in a modals.md heading, because its entry lives in components-v2.md.
    - Mark `derive(Modal)` parts still [planned, Story 4.6] where they are not built.
    - Also update `components-v2.md` §13, the modal-related `limits-cheatsheet.md` rows, and `examples/project-template/AGENTS.md` L38 (`noctis::ui::validate` also takes a `Modal`).

**Acceptance Criteria:**
- Given `noctis-ui`'s modal types, when a modal is built, then its top level accepts only `ModalLabel` or `TextDisplay`, and a Label accepts only the ten modal inputs. A Button, Section, Container, gallery, File, Action Row or message select fails `cargo check` with an E0277 whose message starts with `[MOD-03]`; the trybuild fixtures in task 9 cover this.
- Given any modal select type, when `disabled` is called, then compilation fails (E0599). Given any modal, when it is serialized, then no object has `"type":1` and no type-4 object has a `label` key (snapshot test).
- Given a modal built at runtime, when `noctis::ui::validate(&mut modal, &limits)` runs, then every Matrix error row returns `UiError::Limits` naming the rule id and path. Those rows cover:
  - more than 5 top-level components and a title over 45 (MOD-01/02);
  - a Label over 45 or a description over 100 (MOD-04);
  - Text Input bounds (MOD-05) and File Upload bounds (MOD-06);
  - Radio options outside 2-10 and Checkbox Group options outside 1-10 (MOD-07);
  - `min_values(0)` with `required` (MOD-07/MOD-06/CV2-09);
  - bad or duplicate keys (CID-01/02).
- Given `RouteComponents::modal`, when a modal is built, then its `custom_id` is the Route's encoding (MOD-08). A Route without `Handled` fails with `[CID-04]`, and `Modal` has no other constructor.
- Given the new public API, when `api-snapshot`, `skill-check` and `rules-catalog` run, then all pass. modals.md has one entry per modal component with Limits and Builder, and no `story = "4.5"` entry remains in `rules-pending.toml`.

## Spec Change Log

## Review Triage Log

## Design Notes

- **Why `ModalLabel` and `Modal*Select`.** `noctis::ui::Label` (4.4) and the message selects already own those names. skill-check deduplicates builders by bare name, so the same name in a submodule would merge two entries. AD-12 requires separate message and modal select types. The memlog records this decision.
- **Why keys are strings.** A modal input's `custom_id` is a form-field name echoed in the submission, not a dispatch target. The Route that dispatches is the modal's own. Story 4.6's derive passes field names as keys.
- **Why counts are not typed here.** The AC wants runtime typed errors for 1 and 11 radio options, so constructors must accept them. The compile tier for option counts is `derive(Choices)` in Story 4.6.
- **Checkbox `required`** is unrepresentable (MOD-07 by construction). The `noctis_rules` descriptor still has the field, so runtime lowering always passes `false`. Its rules unit test already covers the violation.
- **Golden example:**
  ```rust
  let modal = Report { id: 7 }.modal("Report a user")
      .push(TextDisplay::new("Tell us what happened."))
      .push(ModalLabel::new("Reason", TextInput::new("reason", TextInputStyle::Paragraph).max_length(500)))
      .push(ModalLabel::new("Severity", RadioGroup::new("sev", [ModalOption::new("Low", "l"), ModalOption::new("High", "h")])))
      .push(ModalLabel::new("Evidence", FileUpload::new("files").required(false).min_values(0).max_values(3)))
      .push(ModalLabel::new("I agree", Checkbox::new("ok")));
  validate(&mut modal, &Limits::default())?;
  ```

## Verification

**Commands** (repo root; CI is paused, so the local gates are the whole check; never make a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (modal tests, snapshots, boundary, UI fixtures, doctests).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, then `-- rules-catalog`, `-- api-snapshot` and `-- skill-check` -- each exits 0.
- `grep -c 'story = "4.5"' xtask/rules-pending.toml` -- prints 0. `test ! -d target/debug` -- true.
