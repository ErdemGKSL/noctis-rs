---
title: 'Story 1.6: Component, modal, message and attachment rules'
type: 'feature'
created: '2026-10-19'
status: 'in-progress'
baseline_revision: '9a45671d04a10abd24f649d6723569bc24b4a0bb'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-1-context.md'
  - '{project-root}/docs/discord-rules.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-rules` has no rule functions for Components V2 trees, modals, legacy message fields, attachments, webhook names or autocomplete responses, so `components!` / `label!` / `rgb!` (Story 4.4), `derive(Modal)` (4.6), the runtime validator (4.3, 4.5), attachments (9.1) and autocomplete (6.x) would each re-implement CV2-03..17, MOD-01..07, MSG-01/03..06, ATT-01..06, WH-04 and INT-07 and could disagree (FR-23, FR-24, FR-29, FR-79, AD-3, AD-4).

**Approach:** Add public modules `noctis_rules::{component, modal, message, attachment}` built like Story 1.5: borrowed, `Copy` descriptor types with `const fn` builders, whole-input validators that report every violation at once, and granular single-value functions for macro literals. Add `command::validate_autocomplete` (INT-07). By-construction rules get `// [ID]`-tagged `compile_fail` doctests. The facade re-exports the modules; the Usage Skill, cheatsheet, catalog and template `AGENTS.md` document them.

## Boundaries & Constraints

**Always:**
- `noctis-rules` stays sync, no I/O, no `async`, no `unsafe`; its only `[dependencies]` entry stays `regex`; `Cargo.lock` unchanged; no new crates anywhere.
- Every new public function takes `&Limits` (AD-3, even where it reads no field) and never calls `Limits::default()` outside `#[cfg(test)]`. Proc-macro callers need no runtime state: every function is a plain, non-generic function; no `const { … }` assertion anywhere (AD-4).
- Every text length is `len::discord_len`. Errors are the existing `Violation` (`Bound`, `Actual`); `Result<_, Vec<Violation>>` never carries an empty `Err`; granular functions return `Vec<Violation>` with path `""` unless stated. Order is deterministic as specified. Functions never truncate: Lenient handling stays with the caller through `Limits::truncates`.
- Existing behavior and tests stay unchanged except: `Limits` gains one field (58 fields), `src/message.rs` is renamed `src/key.rs` (root re-exports `MessageKey`, `Translate` unchanged). `RuleId` count stays 125.
- Every unit/integration test name cites a rule id in snake case (`cv2_03_…`, `mod_05_…`, `att_01_…`, `int_07_…`).
- The public API change updates the Usage Skill, the cheatsheet and the template `AGENTS.md` in the same commit.

**Never:**
- No CID-01 / CID-02 / MOD-08 checks (`custom_id`s are not in the descriptors; Route derive and Story 4.3 own them), no CV2-14 ids, no CV2-01/02, MSG-07, WH-01..03/05, ATT-04/07, MOD-09 code (by construction or later stories), no embed (MSG-02) code, no filename rules, no "Thumbnail is an image" or URL-scheme checks, no Discord-side `max_values ≤ options` check (no catalog row).
- No `noctis-ui` types, twilight types, `serde`, `thiserror`, `bitflags`, `build.rs`, prelude. No new CI gate; no edits to `.github/workflows-disabled/`, `xtask/`, `_bmad/` or the template's `src/`.

## I/O & Edge-Case Matrix

Notation: `L` = `Limits::default()`; "`f=n`" = `L` with field `f` set to `n` (lowered override). `Cnt(n)` = `Actual::Count(n)`, `Txt(s)` = `Actual::Text(s)`, `Btw{a,b}` = `Bound::Between{min:a,max:b}`, `Max(n)` = `Bound::AtMost(n)`, `D(s)` = `Bound::Describe(s)`. `n×x` = `x` repeated n times; `😀` counts 2. Component shorthands: `T(s)` = `Component::Text(s)`; `B` = `Component::Button(Button::new(ButtonStyle::Primary).label("Go"))`; `U` = `Component::Select(Select::new(SelectKind::User))`; `R(..)` = `Component::Row(&[..])`; `C(..)` = `Component::Container(Container::new(&[..]))`; `Sec(texts, acc)` = `Component::Section(Section::new(texts, acc))`; `M(u)` = `Media::new(u)`; `G(..)` = `Component::Gallery(&[..])`; `F(u)` = `Component::File(u)`; `Sp` = `Component::Separator(Separator::new())`; `O(n)` = `Component::Opaque(n)`. "Err[X, Y]" = exactly these rule ids in this order. A `limit+1` row always means: the limit value passes, limit+1 fails.

Paths: top-level node `i` is `kind[i]`; a child is `parent.kind[j]` (`j` = position among all siblings); kinds: `container`, `section`, `text`, `thumbnail`, `gallery`, `file`, `separator`, `row`, `button`, `select`, `opaque`. Section texts `P.text[j]`, accessory `P.accessory`, gallery items `P.item[k]`, select options `P.options[k]`; fields append `.label`, `.url`, `.emoji`, `.placeholder`, `.min_values`, `.max_values`, `.options`, `.default_values`, `.description`, `.accent_color`. Counts of children use the node path itself.

**`component::validate_components(&[Component], &L) -> Result<(), Vec<Violation>>`** (V2 message body)

| Scenario | Input | Expected |
|---|---|---|
| empty | `&[]` | `Ok` |
| AC valid | one Container with `.accent_color(0x5865F2)` holding `T("# Hi")`, `Sp`, `R(B×5)`, `G(M("https://a/x.png"))`, `Sec(&["a"], Accessory::Button(Button::new(ButtonStyle::Primary).label("Go")))`, `F("attachment://a.png")` | `Ok` |
| CV2-03 | 40×`T("a")`; 41×`T("a")`; `components=10` with 11 | ok; `[CV2-03]` path `components`, `Max(40)`, `Cnt(41)`, one note per top-level node `text[0]: 1` … `text[40]: 1`; `Max(10)` `Cnt(11)` |
| CV2-03 nested / CV2-16 | `[O(39), T("a")]`; `[O(40), T("a")]`; `[O(0)]` counts 1; `C(C(T("a")))` counts 3 | ok; CV2-03 `Cnt(41)`; — ; counted although CV2-05 fails |
| CV2-04 | `T(4000×a)`; `T(4001×a)`; `T(2000×😀)`; `T(2001×😀)`; texts 2000 top + 1000 in a container + 1001 in a section; `T(4000×a)` + gallery item description 1024×a + button label 80×a; `text_total_len=10` with `T("abcdefghijk")` | ok; CV2-04 path `components` `Max(4000)` `Cnt(4001)`; ok; `Cnt(4002)`; `Cnt(4001)`; ok (only Text Displays count, wherever placed, misplaced ones too); `Max(10)` `Cnt(11)` |
| CV2-05 | top-level `Thumbnail(M(u))`; top-level `B`; top-level `U`; `C(C(T("a")))`; `C(B)`; `R(T("x"))`; `R(R(B))`; `O(1)` at top, in a container, in a row | path `thumbnail[0]` `D("a Thumbnail only as a Section accessory")` `Txt("thumbnail")`; `button[0]` `D("a Button only in an Action Row or as a Section accessory")` `Txt("button")`; `select[0]` `D("a select only in an Action Row")`; `container[0].container[0]` `D("no Container inside a Container")` `Txt("container")`; `container[0].button[0]` (button rule); `row[0].text[0]` `D("only buttons and selects in an Action Row")` `Txt("text")`; `row[0].row[0]` same, inner row still checked; ok |
| CV2-06 | `R()`; `R(B×5)`; `R(B×6)`; `R(U)`; `R(U,U)`; `R(U,B)`; `R(B,O(1))`; `R(O(1)×6)`; `row_buttons=2` with `R(B×3)` | `row[0]` `Btw{1,5}` `Cnt(0)`; ok; `Cnt(6)`; ok; `row[0]` `D("exactly one select, alone in its row")` `Cnt(2)`; same `Cnt(2)`; ok; `Btw{1,5}` `Cnt(6)`; `Btw{1,2}` `Cnt(3)` |
| CV2-07 | `Sec(&[], acc)`; 1 / 3 texts; 4 texts; `section_texts=1` with 2 | `section[0]` `Btw{1,3}` `Cnt(0)`; ok; `Cnt(4)`; `Btw{1,1}` `Cnt(2)` |
| CV2-08 label | in `R(..)`: label 80×a; 81×a; 40×😀; 41×😀; `Some("")`; `None`; accessory label 81×a; `button_label_len=5` with 6 | ok; `row[0].button[0].label` `Max(80)` `Cnt(81)`; ok; `Cnt(82)`; ok; ok; `section[0].accessory.label`; `Max(5)` `Cnt(6)` |
| CV2-08 link / premium | `ButtonStyle::Link(512×a)`; `Link(513×a)`; `Link("")`; `Premium` with `.label("x")`; `Premium` with `.emoji(true)`; plain `Premium`; `Link` with label | ok; `row[0].button[0].url` `Btw{1,512}` `Cnt(513)`; `Cnt(0)`; `.label` `D("no label on a premium button")` `Txt("x")`; `.emoji` `D("no emoji on a premium button")` `Txt("true")`; ok; ok |
| CV2-09 placeholder / bounds | placeholder 150; 151; `""`; `min_values` 0 / 25 / 26; `max_values` 1 / 25 / 0 / 26; min 3 max 2; min 2 max unset; min 26 max 2; `select_values=5` with max 6 | ok; `row[0].select[0].placeholder` `Max(150)` `Cnt(151)`; ok; ok / ok / `.min_values` `Btw{0,25}` `Cnt(26)`; ok / ok / `.max_values` `Btw{1,25}` `Cnt(0)` / `Cnt(26)`; `row[0].select[0]` `D("min_values ≤ max_values")` `Txt("3 > 2")`; `Txt("2 > 1")` (max defaults to 1, min to 1); only the range violation; `Btw{1,5}` `Cnt(6)` |
| CV2-09 options | `SelectKind::String` with 25 / 26 / 0 options; option label `""` / 100 / 101; value `""` / 101; description 100 / 101 / `Some("")`; `select_options=2` with 3 | ok / `.options` `Btw{1,25}` `Cnt(26)` / `Cnt(0)`; `.options[0].label` `Btw{1,100}` `Cnt(0)` / ok / `Cnt(101)`; `.options[0].value` `Btw{1,100}` `Cnt(0)` / `Cnt(101)`; ok / `.options[0].description` `Max(100)` `Cnt(101)` / ok; `Btw{1,2}` `Cnt(3)` |
| CV2-09 defaults | User min 1 max 3 `default_values(2)` / `(4)` / `(0)`; min 2 max 3 `(1)`; String with 2 options `.default(true)`, min/max unset; String select `.default_values(1)`; min 3 max 2 with `(5)` | ok / `.default_values` `Btw{1,3}` `Cnt(4)` / ok; `Btw{2,3}` `Cnt(1)`; `.default_values` `Btw{1,1}` `Cnt(2)`; `.default_values` `D("default_values only on user, role, mentionable or channel selects")` `Cnt(1)`; only the ordering violation (defaults unchecked) |
| CV2-09 disabled | message select `.disabled(true)` | ok |
| CV2-10 | `G()`; 10 items; 11; item description 1024×a / 1025×a / 512×😀 / 513×😀; `gallery_items=3` with 4 | `gallery[0]` `Btw{1,10}` `Cnt(0)`; ok; `Cnt(11)`; ok / `gallery[0].item[0].description` `Max(1024)` `Cnt(1025)` / ok / `Cnt(1026)`; `Btw{1,3}` `Cnt(4)` |
| CV2-11 | accessory thumbnail description 1024 / 1025; misplaced top-level thumbnail with 1025 | ok / `section[0].accessory.description` CV2-11 `Max(1024)` `Cnt(1025)`; Err[CV2-05, CV2-11 at `thumbnail[0].description`] |
| CV2-12 | `F("attachment://a.png")`; `F("https://x/a.png")`; `F("attachment://")`; `F("")` | ok; `file[0]` `D("an attachment://<filename> URL")` `Txt("https://x/a.png")`; same `Txt("attachment://")`; `Txt("")` |
| CV2-13 | `accent_color` 0 / 0xFFFFFF / 0x1000000 / `u32::MAX`; unset | ok / ok / `container[0].accent_color` `D("a colour from 0x000000 to 0xFFFFFF")` `Txt("0x1000000")` / `Txt("0xFFFFFFFF")`; ok |
| CV2-17 | item url 2048 / 2049 / `""`; accessory thumbnail url 2049; `media_url_len=10` with 11 | ok / `gallery[0].item[0].url` `Btw{1,2048}` `Cnt(2049)` / `Cnt(0)`; `section[0].accessory.url`; `Btw{1,10}` |
| all at once | `[T(4001×a), C(C(), R(B×6)), 38×Sp]` (48 components) | Err[CV2-03, CV2-04, CV2-05 `container[1].container[0]`, CV2-06 `container[1].row[1]`] |

Order: CV2-03, CV2-04, then a pre-order walk; per node: its CV2-05 placement check, then its own checks — Container: CV2-13, then children; Section: CV2-07, then the accessory (button or media checks); Gallery: CV2-10 count, then per item url (CV2-17), description (CV2-10); File: CV2-12; Row: CV2-06, then items; Button: label (length, or premium label), url (Link), emoji (Premium); Select: placeholder, `min_values`, `max_values`, ordering (only when both effective values are in range), options count, per option label/value/description, default values (only when the ordering check ran and passed); Thumbnail (misplaced): url, description. Every node's fields are checked wherever it sits.

**`component::cost(&[Component], &L) -> Cost`** (`Cost { components, text_len }`, `#[non_exhaustive]`, `Copy`, `PartialEq`)

| Input | Expected `components` / `text_len` |
|---|---|
| `&[]`; `C()`; `Sec(&["a"], button)`; `Sec(&["a","b","c"], thumbnail)`; `R(B×5)`; `R(String select with 25 options)`; `G(10 items)`; `O(3)`; `O(0)` | 0/0; 1/0; 3/1; 5/3; 6/0; 2/0; 1/0; 3/0; 1/0 |
| research dashboard: `C(T, Sp, R(B×5), G(1), 5×Sec(T, button), Sp, R(B×4))` | 31 |
| `T("😀"), C(T("ab"))` | 3 / 4 (saturating sums) |

**`component::validate_legacy_components(&[Component], &L) -> Result<(), Vec<Violation>>`** (CV2-15)

| Input | Expected |
|---|---|
| `&[]`; 5×`R(B)`; `[O(1)]` | ok; ok; ok (opaque counts as a row) |
| 6×`R(B)`; `legacy_action_rows=1` with 2×`R(B)` | CV2-15 path `components` `Max(5)` `Cnt(6)`; `Max(1)` `Cnt(2)` |
| `[T("x")]`; `[C(T("a"))]`; `[G(M(u))]` | CV2-15 `text[0]` `D("only Action Rows in a legacy message")` `Txt("text")`; `container[0]` (children not walked); `gallery[0]` |
| `R(B×6)`; `R(button label 81)`; `R(T("x"))`; `R(T(4001×a))` | CV2-06; CV2-08 `row[0].button[0].label`; CV2-05 `row[0].text[0]`; only CV2-05 (CV2-03/CV2-04 are never checked for legacy) |

Order: CV2-15 count, then per top-level node: CV2-15 kind, or the row's checks as in `validate_components`.

**Granular component functions** (path `""`)

| Function | Accepts | Rejects |
|---|---|---|
| `validate_button_label(&str)` CV2-08 | `""`, 80×a, 40×😀 | 81×a `Max(80)` `Cnt(81)`; 41×😀 `Cnt(82)`; `button_label_len=5` + 6 → `Max(5)` |
| `validate_link_url(&str)` CV2-08 | 1, 512 | `""` `Btw{1,512}` `Cnt(0)`; 513 |
| `validate_select_placeholder(&str)` CV2-09 | `""`, 150 | 151 `Max(150)` |
| `validate_gallery_description(&str)` CV2-10 / `validate_thumbnail_description(&str)` CV2-11 | `""`, 1024, 512×😀 | 1025, 513×😀 (`Cnt(1026)`) |
| `validate_media_url(&str)` CV2-17 | 1, 2048 | `""` `Btw{1,2048}`; 2049 |
| `validate_accent_color(u32)` CV2-13 | 0, 0xFFFFFF | 0x1000000 `Txt("0x1000000")`, `u32::MAX` |
| `validate_spacing(u8)` CV2-13 | 1, 2 | 0, 3, 255 → `D("1 (small) or 2 (large)")` `Txt("0")` / `Txt("3")` / `Txt("255")` |
| `Spacing::value`, `ButtonStyle::value` | Small 1, Large 2; Primary 1, Secondary 2, Success 3, Danger 4, Link 5, Premium 6 | — |

**`modal::validate_modal(&Modal, &L) -> Result<(), Vec<Violation>>`** (paths: `title`, `components`, top-level `label[i]` / `text[i]` / `kind[i]` of a `Message` component; Label child `label[i].text_input` / `.select` / `.file_upload` / `.radio_group` / `.checkbox_group` / `.checkbox`)

| Scenario | Input | Expected |
|---|---|---|
| AC valid | title `"Report"`; `Text("Pick")`; Label `"Reason"` + `TextInput::new(Paragraph).min_length(10).max_length(500)`; Label `"Channel"` + `ModalSelect::new(Select::new(SelectKind::Channel))`; Label `"Proof"` + `FileUpload::new().min_values(0).required(false).file_types(&["image", ".pdf"])`; Label `"Severity"` + `RadioGroup::new(&[opt("Low","low"), opt("High","high")])` | `Ok` |
| MOD-01 | title `""` / 45×a / 46×a / 22×😀 / 23×😀; `modal_title_len=10` + 11 | `title` `Btw{1,45}` `Cnt(0)` / ok / `Cnt(46)` / ok / `Cnt(46)`; `Btw{1,10}` `Cnt(11)` |
| MOD-02 | 0 / 1 / 5 / 6 components; `modal_components=2` + 3 | `components` `Btw{1,5}` `Cnt(0)` / ok / ok / `Cnt(6)`; `Btw{1,2}` `Cnt(3)` |
| MOD-03 | `Message(B-component)`; `Message(C(..))`; `Message(T("x"))`; modal select `.disabled(true)` | `button[0]` `D("only Labels or Text Displays at the top level of a modal")` `Txt("button")`, not walked; `container[0]`; ok (same as `Text`); `label[0].select.disabled` `D("no disabled in a modal")` `Txt("true")` |
| MOD-04 | Label text `""` / 45 / 46; description 100 / 101 / `Some("")`; `label_len=3` + 4 | `label[0].label` `Btw{1,45}` `Cnt(0)` / ok / `Cnt(46)`; ok / `label[0].description` `Max(100)` `Cnt(101)` / ok; `Btw{1,3}` `Cnt(4)` |
| MOD-05 | `min_length` 0 / 4000 / 4001; `max_length` 1 / 4000 / 0 / 4001; min 5 max 5; min 10 max 5; min 4001 max 5; min 10 only; `value` 4000 / 4001; `placeholder` 100 / 101; `text_input_len=10` + max 11 | ok / ok / `label[0].text_input.min_length` `Btw{0,4000}` `Cnt(4001)`; ok / ok / `.max_length` `Btw{1,4000}` `Cnt(0)` / `Cnt(4001)`; ok; `label[0].text_input` `D("min_length ≤ max_length")` `Txt("10 > 5")`; only the range violation; ok (no ordering without both); ok / `.value` `Max(4000)` `Cnt(4001)`; ok / `.placeholder` `Max(100)` `Cnt(101)`; `Btw{1,10}` `Cnt(11)` |
| MOD-06 | min 0 + `required(false)`; min 0 + required (default); min 10 max 10; min 11; max 0; max 11; min 3 max unset; `file_types` 10 / 11 entries; entries `"pdf"`, `"."`, `".tar.gz"`, `"Image"`; `".PNG"`, `"video"`; `file_upload_values=2` + max 3 | ok; `label[0].file_upload.min_values` `D("min_values ≥ 1 when required")` `Cnt(0)`; ok; `.min_values` `Btw{0,10}` `Cnt(11)`; `.max_values` `Btw{1,10}` `Cnt(0)`; `Cnt(11)`; `label[0].file_upload` `D("min_values ≤ max_values")` `Txt("3 > 1")`; ok / `.file_types` `Max(10)` `Cnt(11)`; one MOD-06 each at `.file_types[k]` `D(FILE_TYPE)` (the OPT-13 text) `Txt(entry)`; ok; `Btw{1,2}` `Cnt(3)` |
| MOD-07 radio | 1 / 2 / 10 / 11 options; option label `""` / 100 / 101; value `""` / 101; description 101; `radio_options=3` + 4; `modal_option_text_len=5` + label 6 | `label[0].radio_group.options` `Btw{2,10}` `Cnt(1)` / ok / ok / `Cnt(11)`; `.options[0].label` `Btw{1,100}` `Cnt(0)` / ok / `Cnt(101)`; `.options[0].value` same; `.options[0].description` `Max(100)` `Cnt(101)`; `Btw{2,3}` `Cnt(4)`; `Btw{1,5}` `Cnt(6)` |
| MOD-07 checkbox group | 0 / 1 / 10 / 11 options; min 0 + `required(false)`; min 0 + required; min 11; max 0 / 11; 2 options + min 3, max unset; min 2 max 1; 11 options, min/max unset; `checkbox_group_values=3` + max 4 | `label[0].checkbox_group.options` `Btw{1,10}` `Cnt(0)` / ok / ok / `Cnt(11)`; ok; `.min_values` `D("min_values ≥ 1 when required")` `Cnt(0)`; `.min_values` `Btw{0,10}` `Cnt(11)`; `.max_values` `Btw{1,10}` `Cnt(0)` / `Cnt(11)`; `label[0].checkbox_group` `D("min_values ≤ max_values")` `Txt("3 > 2")` (max defaults to the option count, min to 1); `Txt("2 > 1")`; only the options count (default max 11 out of range, ordering skipped); `Btw{1,3}` `Cnt(4)` |
| MOD-07 checkbox | `Checkbox::new()`; `.required(true)` | ok; `label[0].checkbox.required` `D("a Checkbox is never required")` `Txt("true")` |
| modal select | `Select` fields as in `validate_components` (CV2-09) at `label[0].select…`; required (default) + `min_values(0)`; `.required(false).min_values(0)` | same violations; CV2-09 `label[0].select.min_values` `D("min_values ≥ 1 when required")` `Cnt(0)`; ok |
| all at once | title 46×a, 6 components: `Message(B)`, Label `""` + Checkbox required, 4×`Text("x")` | Err[MOD-01, MOD-02, MOD-03, MOD-04, MOD-07] |

Order: MOD-01, MOD-02, then per top-level component in input order: MOD-03 for a `Message`; for a Label: label, description, then the child — TextInput: `min_length`, `max_length`, ordering, `value`, `placeholder`; Select: MOD-03 `disabled`, CV2-09 checks, required/`min_values`; FileUpload: `min_values`, `max_values`, ordering, required/`min_values`, `file_types` count, entries; RadioGroup: options count, per option label/value/description; CheckboxGroup: options count, `min_values`, `max_values`, ordering, required/`min_values`, per option; Checkbox: `required`. Effective defaults: select and File Upload min 1 / max 1, Checkbox Group min 1 / max = option count; ordering runs only when both effective values are in range.

**Granular modal functions** (path `""`): `validate_modal_title` (MOD-01, `""`→`Btw{1,45}` `Cnt(0)`, 45 ok, 46), `validate_label_text` (MOD-04 `Btw{1,45}`), `validate_label_description` (MOD-04 `Max(100)`, `""` ok), `validate_text_input(&TextInput)` (MOD-05, paths `min_length` / `max_length` / `value` / `placeholder`, ordering `""`, rows as above), `validate_text_input_value(&str)` (MOD-05 `Max(4000)`, 4000 ok, 4001, `""` ok).

**`attachment`**

| Function / scenario | Input | Expected |
|---|---|---|
| `validate_attachments(&[AttachmentDecl], json_len, file_limit: Option<usize>, &L) -> Result<(), Vec<Violation>>` empty | `&[]`, 0, `None` | `Ok` |
| ATT-03 | 10 / 11 files of 1 byte; `attachments=2` + 3 | ok / `attachments` `Max(10)` `Cnt(11)`; `Max(2)` `Cnt(3)` |
| ATT-01 | `a.png` 20_971_520 / 20_971_521 bytes; `file_limit Some(10_485_760)` + 10_485_761; `Some(104_857_600)` + 52_428_800; `attachment_bytes=1000` + 1001 | ok / `attachments[0].size` `Max(20971520)` `Cnt(20971521)` note ``file `a.png` ``; `Max(10485760)`; ok for ATT-01, ATT-02 `Cnt(52428800)`; `Max(1000)` `Cnt(1001)` |
| ATT-06 | description 1024 / 1025 / 512×😀 / 513×😀 / `Some("")` | ok / `attachments[0].description` `Max(1024)` `Cnt(1025)` note ``file `a.png` `` / ok / `Cnt(1026)` / ok |
| ATT-02 | two files 13_107_200 each + json 0 / json 1; sizes `usize::MAX` + 1 | ok / `request` `Max(26214400)` `Cnt(26214401)`; `Cnt(usize::MAX)` (saturating) |
| order | 11 files, #0 of 26_214_401 bytes with 1025-char alt text, the rest 1 byte | Err[ATT-03, ATT-01, ATT-06, ATT-02] (ATT-03, per file ATT-01 then ATT-06, then ATT-02) |
| `validate_file_size(size, file_limit, &L)` ATT-01 | as the ATT-01 row, path `""`, no note | same bounds |
| `validate_attachment_description(&str, &L)` ATT-06 | `""`, 1024 / 1025 | ok / `Max(1024)` `Cnt(1025)` |
| `validate_attachment_ref(url, &[AttachmentDecl], &L) -> Vec<Violation>` ATT-05 | `"attachment://a.png"` with `a.png` / with only `b.png`; `"attachment://"`; `"attachment://A.png"` with `a.png`; `"https://x/y.png"`; `"ATTACHMENT://a.png"` | `[]` / ATT-05 path `""` `D("attachment://<name> of a file in this message")` `Txt(url)`; ATT-05; ATT-05 (case-sensitive); `[]`; `[]` (not an attachment reference) |

**`message`**

| Function | Accepts | Rejects |
|---|---|---|
| `validate_content(&str, &L)` MSG-01, path `content` | `""`, 2000×a, 1000×😀 | 2001×a `Max(2000)` `Cnt(2001)`; 1001×😀 `Cnt(2002)`; `content_len=5` + 6 |
| `validate_mentions(&AllowedMentions, &L)` MSG-04 | `AllowedMentions::new()`; `users(MentionIds::Only(100 ids))`; `MentionIds::All`; `Only(&[])` | users 101 → `allowed_mentions.users` `Max(100)` `Cnt(101)`; roles 101 → `allowed_mentions.roles`; both → [users, roles]; `mention_users=1` + 2 |
| MSG-05 default | `AllowedMentions::default() == AllowedMentions::new()`: users / roles `MentionIds::None`, `everyone` / `replied_user` false | — |
| `validate_nonce(&str, &L)` MSG-06, path `nonce` | `""`, 25×a, 12×😀+`a` | 26×a `Max(25)` `Cnt(26)`; 13×😀 `Cnt(26)` |
| `validate_sticker_ids(count, &L)` MSG-06, path `sticker_ids` | 0, 3 | 4 `Max(3)` `Cnt(4)`; `sticker_ids=1` + 2 |
| `validate_webhook_name(&str, &L)` WH-04, path `name` | 1, 80×a, 40×😀 | `""` `Btw{1,80}` `Cnt(0)`; 81 `Cnt(81)`; 41×😀 `Cnt(82)`; `webhook_name_len=10` + 11 |

**`command::validate_autocomplete(&[Choice], OptionType, &L) -> Vec<Violation>`** (INT-07; `I53` = 2^53−1)

| Input | Expected |
|---|---|
| `&[]`, String; 25 `Str` choices | `[]`; `[]` |
| 26 choices; `autocomplete_choices=2` + 3 | `[INT-07]` path `choices` `Max(25)` `Cnt(26)`, help `Noctis sends the first 25 choices and logs this as a warning`; `Max(2)` `Cnt(3)`, help with `2` |
| 26 choices, index 25 named `""`; index 0 named `""` | only INT-07 (dropped choices unchecked); Err order [INT-07, OPT-09 `choices[0]` `Btw{1,100}` `Cnt(0)` note `in the choice name`] |
| name 101; `name_localized(&[101×a])`; String + `Int(1)`; String + `Str(101×a)`; Integer + `Int(I53+1)`; Number + `Float(NAN)`; Number + `Int(1)` | OPT-09 `Cnt(101)`; OPT-09 note `in localized choice name #0`; OPT-09 `D("a choice value of the option's type")` `Txt("integer")` note `option type STRING`; OPT-09 `Max(100)` note `in the choice value`; OPT-03 `choices[0]`; OPT-04 `Txt("NaN")`; `[]` |
| Boolean kind (any choices, even 26) | `[OPT-11]` path `choices` `D("autocomplete only on STRING, INTEGER or NUMBER options")` `Txt("BOOLEAN")`, nothing else |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/lib.rs` (edit): crate doc list l.6-20; `mod message;` → `mod key;` and `pub use key::{MessageKey, Translate}`; add `pub mod attachment; pub mod component; pub mod message; pub mod modal;`.
- `crates/noctis-rules/src/message.rs` → `git mv` to `src/key.rs` (content unchanged, `MessageKey`, `Translate`); the new `src/message.rs` holds the message rules.
- `crates/noctis-rules/src/limits.rs` (edit): `limits!` table l.79-137 (add `modal_option_text_len = 100, "MOD-07", "characters in a Radio or Checkbox Group option label, value or description.";` right after `checkbox_group_values`), `defaults_match_the_catalog` array (57 → 58, add `(l.modal_option_text_len, 100)` in table order), `field_table_has_57_unique_fields_with_catalog_ids` → `…_58_…`. `cheatsheet_limits_table_matches_fields` reads the skill cheatsheet, so the cheatsheet row must land in the same order.
- `crates/noctis-rules/src/rule.rs` (read-only): `Violation::new/with_note/with_help`, `Bound`, `Actual`, ids `CV2_03..CV2_17`, `MOD_01..07`, `MSG_01..06`, `ATT_01..06`, `WH_04`, `INT_07`, `OPT_03/04/09/11` exist. `src/len.rs` (reuse `discord_len`).
- `crates/noctis-rules/src/command/option.rs` (edit): reuse `check_choice_names` (l.647), `check_choice_value` (l.674) with `ValidBounds { min: None, max: None }` (l.509), `OptionType::is_choosable`/`name`; make `is_file_type` (l.769) and `FILE_TYPE` (l.19) `pub(crate)` for MOD-06; add `pub fn validate_autocomplete`. `src/command/mod.rs` (edit): re-export `validate_autocomplete`, extend the module doc rule list (l.1-3) with INT-07 and an "Autocomplete" paragraph.
- Style to copy: `command/option.rs` (`#[non_exhaustive]` descriptor structs with pub fields, `const fn new`, by-value `#[must_use] const fn` setters, `out: &mut Vec<Violation>` helpers, test helpers `between`/`describe`), `tests/options.rs` (integration test shape), `tests/purity.rs` / `tests/catalog.rs` (read-only; must keep passing).
- `crates/noctis/src/lib.rs` (edit): `pub mod rules` doc list l.22-38, `pub use noctis_rules::{attachment, command, component, message, modal};`, one new doctest.
- `docs/discord-rules.md` (edit): MOD-07 row (l.139 in §10) and "Unconfirmed values" table (l.543-556).
- Skill (`examples/project-template/.claude/skills/noctis/references/`): `components-v2.md` (after §14 l.224), `modals.md` (after §6), `commands.md` §4 Autocomplete (l.309-332), `limits-cheatsheet.md` (Limits table l.~84, Rules types table l.~100-115, MOD-07 row), `troubleshooting.md` (Runtime section l.73). `examples/project-template/AGENTS.md` Conventions l.36-37.
- Previous story (1.5, done): descriptor/validator conventions, `Result<(), Vec<Violation>>` for whole inputs and `Vec<Violation>` for single values, violation-order docs on each function, `compile_fail` doctests tagged `// [ID]`, rejected findings (no checks without a catalog row) apply here too.

## Tasks & Acceptance

**Execution:**
- `crates/noctis-rules/src/key.rs` -- `git mv src/message.rs src/key.rs`; update `lib.rs` (`mod key;`, re-exports, module list, doc list naming the four new modules and their rule ids) -- frees `noctis_rules::message` for the message rules.
- `crates/noctis-rules/src/limits.rs` -- add `modal_option_text_len` and update the two count tests -- MOD-07 option text needs a configurable limit.
- `crates/noctis-rules/src/component.rs` (new) -- types (all `Clone, Copy, Debug, PartialEq`; structs `#[non_exhaustive]` with pub fields, `const fn new` and by-value `#[must_use] const fn` setters named after the fields): `enum Component<'a> { Container(Container<'a>), Section(Section<'a>), Text(&'a str), Thumbnail(Media<'a>), Gallery(&'a [Media<'a>]), File(&'a str), Separator(Separator), Row(&'a [Component<'a>]), Button(Button<'a>), Select(Select<'a>), Opaque(usize) }` with `const fn kind(&self) -> &'static str` (the path names); `Container { children, accent_color: Option<u32>, spoiler: bool }` (`new(children)`); `Section { texts: &'a [&'a str], accessory: Accessory<'a> }` (`new(texts, accessory)`); `enum Accessory<'a> { Button(Button<'a>), Thumbnail(Media<'a>) }`; `Media { url, description: Option<&'a str>, spoiler }` (`new(url)`); `Separator { divider: bool, spacing: Spacing }` (`new()` = divider true, Small; also `Default`); `enum Spacing { Small, Large }` (`Eq`, `Hash`, `Default` Small, `const fn value -> u8`); `enum ButtonStyle<'a> { Primary, Secondary, Success, Danger, Link(&'a str), Premium }` (`const fn value -> u8`); `Button { style, label: Option<&'a str>, emoji: bool }` (`new(style)`); `enum SelectKind<'a> { String(&'a [SelectOption<'a>]), User, Role, Mentionable, Channel }`; `Select { kind, placeholder: Option<&'a str>, min_values: Option<usize>, max_values: Option<usize>, default_values: usize, disabled: bool }` (`new(kind)`); `SelectOption { label, value, description: Option<&'a str>, default: bool }` (`new(label, value)`); `Cost { components: usize, text_len: usize }`. Functions per the matrix: `validate_components`, `validate_legacy_components`, `cost`, `validate_button_label`, `validate_link_url`, `validate_select_placeholder`, `validate_gallery_description`, `validate_thumbnail_description`, `validate_media_url`, `validate_accent_color`, `validate_spacing`; a `pub(crate)` select checker (path prefix argument) reused by `modal`. Module docs: the path format, the order, what each rule means, that `Opaque(n)` stands for raw components (CV2-16) and macro holes, that CV2-05/06/07/10 are also enforced by types in `noctis-ui`, and that the functions never truncate. `compile_fail` doctests: `let _ = Section::new(&["a"]); // [CV2-07]` (no section without an accessory), `let _ = Button::new(ButtonStyle::Link("u")).custom_id("x"); // [CV2-08]`, `let _ = Spacing::Medium; // [CV2-13]`. Unit tests for every component row plus proptests: (a) `n` in `0..=60` `T("a")` nodes → CV2-03 iff `n > 40`; (b) random strings in 0..=3 texts → CV2-04 iff the `discord_len` sum exceeds 4000 and `cost().text_len` equals that sum.
- `crates/noctis-rules/src/modal.rs` (new) -- types: `Modal { title, components: &'a [ModalComponent<'a>] }` (`new(title, components)`); `enum ModalComponent<'a> { Label(Label<'a>), Text(&'a str), Message(Component<'a>) }`; `Label { label, description: Option<&'a str>, child: LabelChild<'a> }` (`new(label, child)`); `enum LabelChild<'a> { TextInput(TextInput<'a>), Select(ModalSelect<'a>), FileUpload(FileUpload<'a>), RadioGroup(RadioGroup<'a>), CheckboxGroup(CheckboxGroup<'a>), Checkbox(Checkbox) }`; `enum TextInputStyle { Short, Paragraph }` (`value` 1/2); `TextInput { style, min_length, max_length: Option<usize>, required: bool, value, placeholder: Option<&'a str> }` (`new(style)`, required true); `ModalSelect { select: Select<'a>, required: bool }` (`new(select)`, required true); `FileUpload { min_values, max_values: Option<usize>, required: bool, file_types: &'a [&'a str] }` (`new()`); `ModalOption { label, value, description: Option<&'a str>, default: bool }` (`new(label, value)`); `RadioGroup { options, required }`; `CheckboxGroup { options, min_values, max_values, required }`; `Checkbox { default: bool, required: bool }` (`new()`, required false). Functions per the matrix: `validate_modal`, `validate_modal_title`, `validate_label_text`, `validate_label_description`, `validate_text_input`, `validate_text_input_value`. `compile_fail` doctest `let _ = LabelChild::Button; // [MOD-03]`. Unit tests for every modal row.
- `crates/noctis-rules/src/message.rs` (new) -- `enum MentionIds<'a> { #[default] None, All, Only(&'a [u64]) }` (`Default`); `#[non_exhaustive] struct AllowedMentions<'a> { users, roles: MentionIds<'a>, everyone, replied_user: bool }` (`const fn new()` = none, `Default`, setters); functions `validate_content`, `validate_mentions`, `validate_nonce`, `validate_sticker_ids`, `validate_webhook_name`. Module docs state MSG-02 (no embed API in v1; a later embed builder must add its limits here), MSG-03 and MSG-05 (by construction, defaults ping nobody). `compile_fail` doctest `let _ = MentionIds::All(&[1_u64]); // [MSG-03]`; unit test `msg_05_default_parses_nothing`. Unit tests for every message row.
- `crates/noctis-rules/src/attachment.rs` (new) -- `#[non_exhaustive] struct AttachmentDecl<'a> { filename, size: usize, description: Option<&'a str> }` (`new(filename, size)`, `.description`); functions `validate_attachments`, `validate_file_size`, `validate_attachment_description`, `validate_attachment_ref`. Docs: an interaction's `attachment_size_limit`, when passed as `file_limit`, replaces `Limits::attachment_bytes`; ATT-02 total = file sizes + `json_len` (saturating); ATT-04/ATT-07 are by construction. Unit tests for every attachment row.
- `crates/noctis-rules/src/command/option.rs`, `command/mod.rs` -- `validate_autocomplete` and its re-export, `pub(crate)` file-type helper; unit tests for every INT-07 row.
- `crates/noctis-rules/tests/components.rs` (new) -- the epics ACs through the public API: one test per listed rule asserting the limit passes and limit + 1 fails (CV2-03 40/41, CV2-04 4000/4001, CV2-06 5/6 buttons and 1/2 selects, CV2-07 3/4 texts, CV2-08 label 80/81 and url 512/513 and premium label, CV2-09 placeholder 150/151 and options 25/26 and min/max and `default_values`, CV2-10 10/11 items and 1024/1025, CV2-11, CV2-13 0xFFFFFF/0x1000000 and spacing 2/3, CV2-15 5/6, MOD-01..07, MSG-01, MSG-04, MSG-06, ATT-01..03, ATT-05, ATT-06, WH-04, INT-07); and `lowered_override_*` tests where `components = 10`, `button_label_len = 5`, `modal_title_len = 10`, `attachment_bytes = 1000`, `webhook_name_len = 10` reject what defaults accept.
- `crates/noctis/src/lib.rs` -- doc list and re-exports; doctest: 41 `Component::Text("a")` nodes give `RuleId::CV2_03` with path `components`, and `validate_modal_title(&46×a, &Limits::default())[0].rule == RuleId::MOD_01`.
- `docs/discord-rules.md` -- MOD-07 limit text adds "; option `label` / `value` 1-100, `description` ≤ 100"; Unconfirmed rows: `| CV2-08, CV2-09, MOD-01, MOD-04, MOD-07 | The minimum length of required text fields (link url, select and modal option label/value, modal title, Label text) is not stated. | At least 1 character; optional fields (button label, placeholders, descriptions, alt text) may be empty. |`, `| CV2-09 | The minimum String Select option count, and when the default-value count is checked. | 1-25 options; the default count (default_values, or default-flagged options) must lie in [min_values, max_values] only when there is at least one. |`, `| ATT-01 | Whether a lowered Limits::attachment_bytes applies when the interaction reports attachment_size_limit. | The interaction's value, when given, replaces the field. |`, `| MOD-06 | Allowed file_types entry syntax. | Same as OPT-13. |`.
- `examples/project-template/.claude/skills/noctis/references/components-v2.md` -- new "## 15. Rule functions (`noctis::rules::component`, `message`, `attachment`)": every type and function above, path format, violation order, `Opaque(n)`, "functions report, the Responder truncates in Lenient mode", `cost`; a short example.
- `examples/project-template/.claude/skills/noctis/references/modals.md` -- new "## 7. Rule functions (`noctis::rules::modal`)" covering every modal type and function, paths, effective defaults and order.
- `examples/project-template/.claude/skills/noctis/references/commands.md` -- §4: `validate_autocomplete(&choices, OptionType::String, &limits)` (INT-07 count reported for the caller to truncate, OPT-09/03/04 per kept choice, OPT-11 on other types).
- `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md` -- Limits table row `| \`modal_option_text_len\` | MOD-07 | 100 | characters in a Radio or Checkbox Group option label, value or description |` after `checkbox_group_values`; Rules types rows for `component::…`, `modal::…`, `message::…`, `attachment::…`, `command::validate_autocomplete`; MOD-07 row adds "option label/value 1–100, description ≤ 100".
- `examples/project-template/.claude/skills/noctis/references/troubleshooting.md` -- Runtime rows for `[CV2-03]`, `[CV2-04]`, `[CV2-06]`, `[CV2-09]`, `[MOD-05]`, `[ATT-01]`/`[ATT-02]`, `[ATT-05]`, `[INT-07]` quoting real `Violation` text and the fix.
- `examples/project-template/AGENTS.md` -- convention line: dynamic component trees, modals, attachments and webhook names can be pre-checked with `noctis::rules::{component, modal, attachment, message}`; never hard-code their limits.

**Acceptance Criteria:**
- Given `noctis-rules`, when `cargo test --release -p noctis-rules` runs, then every matrix row passes as a unit, property, doctest or integration test whose name cites its rule id, the new `compile_fail` doctests fail only on their `// [ID]` lines, `RuleId::ALL.len() == 125`, `FIELDS.len() == 58`, and `cargo tree -p noctis-rules -e normal --depth 1` lists only `regex`.
- Given a lowered `Limits` override, when any new function receives it, then it checks against the override (integration `lowered_override_*` tests), and `grep -rn "Limits::default()" crates/noctis-rules/src` matches only `#[cfg(test)]` code, doc comments and the `Default` impl.
- Given a bot crate depending only on `noctis`, when it calls `noctis::rules::{component, modal, message, attachment}` and `noctis::rules::command::validate_autocomplete`, then it compiles and the facade doctest shows CV2-03 and MOD-01.
- Given the Usage Skill, when grepped, then each of `validate_components`, `validate_legacy_components`, `cost`, `Component`, `Opaque`, `validate_button_label`, `validate_accent_color`, `validate_spacing`, `validate_modal`, `ModalComponent`, `LabelChild`, `validate_text_input`, `validate_content`, `AllowedMentions`, `validate_webhook_name`, `validate_attachments`, `validate_attachment_ref`, `validate_autocomplete` and `modal_option_text_len` appears in `references/`.
- Given `cargo run --release -p xtask -- deps-check`, when it runs, then it exits 0.

## Spec Change Log

## Review Triage Log

## Design Notes

**Decisions (logged to the memlog):**
- A descriptor tree in `noctis-rules`, not per-field functions only: Story 4.3 lowers its `noctis-ui` tree into `&[Component]` and Story 4.4 lowers literal syntax (holes → `Opaque(1)`, spreads omitted) into the same tree, so both get one count, one path format and one order. Granular functions exist for single literals (`label!`, `placeholder!`, `rgb!`, `derive(Modal)` attributes, pre-fill). Descriptors carry no `custom_id` or numeric id (CID-01/02, CV2-14 belong to Routes and Story 4.3).
- Placement is checked here too (CV2-05, MOD-03) because `components!` must report it with a span; the typed builders still make it unrepresentable for authors.
- `Opaque(n)` counts `max(n, 1)`, is allowed at any position and counts as one row item; a select still must be alone in its row.
- `src/message.rs` (MessageKey, Translate) is private, so renaming it to `src/key.rs` changes no public path.
- Effective defaults (select / File Upload min 1 max 1, Checkbox Group max = option count) drive the ordering checks because Discord applies them; checks that depend on an out-of-range value are skipped, as in Story 1.5.
- `Limits::modal_option_text_len` is new (MOD-07 row extended in the catalog) so every numeric limit stays in `Limits`.
- INT-07 is reported as a `Violation` the caller logs as a warning after truncating to `limits.autocomplete_choices`; only the kept choices are checked.

## Verification

**Commands** (repo root; CI is paused, these local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- expected: exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- expected: exit 0.
- `cargo test --release --workspace` -- expected: exit 0 (unit, property, integration, catalog, purity, cheatsheet tests and doctests).
- `cargo hack test --release --workspace --each-feature` -- expected: exit 0.
- `cargo run --release -p xtask -- deps-check` -- expected: exit 0.
- `cargo tree -p noctis-rules -e normal --depth 1` -- expected: `noctis-rules` and `regex v1.13.x` only.
- `cargo deny check` -- expected: exit 0.
- `cargo +1.91 check --release --workspace` -- expected: exit 0.
- `test ! -d target/debug` -- expected: true.
