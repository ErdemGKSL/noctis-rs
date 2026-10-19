---
title: 'Story 4.3: Runtime Limit validator with path-precise errors'
type: 'feature'
created: '2026-10-19'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-4-context.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/examples/project-template/.claude/skills/noctis/references/components-v2.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** `noctis-ui` builds typed `V2Message`/`LegacyMessage` values, but nothing checks a runtime-built message against the Discord Limits before it is sent. Array constructors are unbounded (Section texts, gallery items, row buttons), and text, labels, URLs, option counts, duplicate `custom_id`s, `attachment://` references, alt text, legacy `content`, mention id counts and empty bodies are all unchecked. So a dynamic message can still get a 400 from Discord (FR-24, FR-78 runtime half; AD-3, AD-4; CV2-03, CV2-04, CV2-06..CV2-11, CV2-13, CV2-14, CID-02, ATT-05, ATT-06, plus MSG-01, MSG-04 and the new MSG-09).

**Approach:** Add a `limits` module to `noctis-ui` (new normal deps `noctis-rules`, `tracing`) with these items:
- `validate(&mut msg, &limits) -> Result<CostReport, UiError>` and `cost(&msg, &limits) -> CostReport`, generic over a sealed `Validate` trait implemented for `V2Message` and `LegacyMessage`.
- Both lower the typed tree into `noctis_rules::component` descriptors and call `validate_components` / `validate_legacy_components` / `cost`, so paths, counts and order match `components!` exactly.
- On top of that they add CID-02, ATT-05, ATT-06, MSG-01, MSG-04 and MSG-09.
- Lenient mode truncates display text with `…` before checking. Missing alt text on non-`Rendered` images is a warning.
- Every warning is returned in the `CostReport` and logged with `tracing::warn!`.
- The facade re-exports the new items. The catalog, the rules crate (MSG-09), the skill and the template are updated in the same commit.

## Boundaries & Constraints

**Always:**
- Layering per the spine. The `noctis-ui → noctis-rules` edge is already allowed (`xtask/src/deps_check.rs` L62).
- `tracing` 0.1.44 (verified on crates.io: latest stable, MSRV well below 1.91). Change the root workspace entry to `tracing = { version = "0.1.44", default-features = false, features = ["std"] }` (no `attributes`, so no syn-2 duplicate) and fix the comment above `[workspace.dependencies]`, which says only TLS crates and takumi turn defaults off. Add `noctis-rules.workspace = true` and `tracing.workspace = true` to `crates/noctis-ui/Cargo.toml`. No other new crate.
- `noctis-rules` stays sync and I/O-free. Every limit comes from the `&Limits` argument, never from a constant. `discord_len` is the only length function and `noctis_rules::len::truncate` the only truncation.
- Paths follow `noctis_rules::component`'s scheme verbatim: `kind[i]`, `parent.kind[j]`, `.accessory`, `.item[k]`, `.options[k]`, `.label`, `.url`, `.description`, `.placeholder`, `.min_values`, `.max_values`, `.default_values`. Kinds are `container`, `section`, `text`, `thumbnail`, `gallery`, `file`, `separator`, `row`, `button`, `select`.
- Paths of the noctis-ui-only checks:
  - `P.custom_id` (CID-02)
  - `P.url` (ATT-05, where P is the File node, the Thumbnail accessory or the gallery item)
  - `attachments[k].description` (ATT-06; k is the index in `AttachmentSet::iter()` order)
  - `content` (MSG-01, legacy MSG-09)
  - `components` (V2 MSG-09)
  - `allowed_mentions.users` / `.roles` (MSG-04)
  - `P.text[k]` (Section text k, used only in notes)
- Violation order in `UiError::Limits`, from first to last:
  1. The `noctis-rules` component result in its own order.
  2. CID-02, pre-order of the duplicate.
  3. ATT-05, pre-order.
  4. ATT-06, set order.
  5. MSG-01 (legacy only).
  6. MSG-09.
  7. MSG-04 (users, then roles).
- Strict mode (the default) never mutates the message. Lenient mode mutates only these display-text fields, and only when `limits.truncates(rule)`:
  - Text Display content (CV2-04 total)
  - button label (Route, Link) (CV2-08)
  - select placeholder, String Select option label and description (CV2-09)
  - gallery item alt (CV2-10)
  - Thumbnail alt (CV2-11)
  - `Attachment` alt (ATT-06)
  - legacy content (MSG-01)

  Everything else always fails: counts, URLs, option values, min/max values, ids, CID-02, ATT-05, MSG-04, MSG-09.
- Follow the 4.1/4.2 conventions: every public type is `Debug + Clone + Send + Sync`, is documented and has a doctest. Every new public item is named in the Usage Skill (skill-check mention check). Re-bless `xtask/public-api.txt` and write the new hash into `SKILL.md` in the same commit.
- All cargo commands use `--release`. Never edit `.cargo/config.toml` or `_bmad/`.

**Never:**
- Do not change the descriptor types or existing check semantics in `noctis_rules::component`. The only noctis-rules change is the new MSG-09 functions plus the RuleId entry and the counts.
- No `label!`, `rgb!`, `components!`, `Rgb::try_from`, `try_*` constructors or arity traits (4.4). No modal validation (4.5). No Responder, HTTP, attachment byte or count budget (ATT-01..03 belong to core), Views or Paginator (Epics 7/8). No error for rendered images without alt (9.1). No raw components (9.2).
- No `unsafe`, no arena crate (`bumpalo`/`typed-arena`), no `const` assert in generic code.
- Do not implement `Validate` for `Components` or any type outside the crate (sealed).
- Do not drop or reorder violations from `noctis-rules`.

## I/O & Edge-Case Matrix

Notation:
- `L` = `Limits::default()`. `Len` = `L` with `mode = LimitMode::Lenient`.
- `v(m)` = `validate(&mut m, &L)`. `c(m)` = `cost(&m, &L)`.
- `B` = a test Route button (`route!` helper as in `tests/components.rs`, implementing `Handled`). `B(n)` = distinct payloads.
- `T(s)` = `TextDisplay::new(s)`. `M(cs)` = `V2Message::new(cs)`.
- `"a"*n` = `"a".repeat(n)`. `😀*n` = n emoji (2 units each).
- "Err[R @ p, bound, actual]" means `UiError::Limits` whose violation (at the stated position, else the only one) has rule R, path p, `Bound` and `Actual`.

| # | Scenario | Input / State | Expected Output / Behavior | Error Handling |
|---|---|---|---|---|
| 1 | Minimal V2 | `M([T("hi")])` | `Ok(CostReport{components:1, text_chars:2, warnings:[]})`; `c` equals it | none |
| 2 | CV2-03 max | 40 components: `Container` + 39 `T("a")` children (1+39) | Ok, `components == 40` | none |
| 3 | CV2-03 max+1 | 41 top-level `T("a")` | Err[CV2-03 @ `components`, AtMost(40), Count(41)] with 41 notes `text[i]: 1` | error |
| 4 | CV2-03 nested | Container with 8 rows × 5 `B` | count 1+8×6=49; Err CV2-03 (49); lenient also Err | error, structural |
| 5 | CV2-03 lowered limit | `limits.components = 3`, `M([T,T,T,T])` | Err CV2-03 AtMost(3) Count(4) | error |
| 6 | CV2-03 accessory counts | Section with 3 texts + Thumbnail | cost 5 | none |
| 7 | CV2-04 max | `T("a"*4000)` | Ok, `text_chars 4000` | none |
| 8 | CV2-04 max+1 | `T("a"*4000)`, `T("b")` | Err[CV2-04 @ `components`, AtMost(4000), Count(4001)] | error |
| 9 | CV2-04 unicode | `T(😀*2000)` ok (4000); `T(😀*2000 + "a")` Err Count(4001); `T("é"*4000)` ok | as stated | error on overflow |
| 10 | CV2-04 nested and section | texts in Container and Section count | `text_chars` = sum over top, container and section texts | none |
| 11 | CV2-06 0 / 6 buttons | `ActionRow::buttons::<Button,0>([])`; 6 `B` | Err[CV2-06 @ `row[0]`, Between{1,5}, Count(0/6)]; 5 ok | error, structural |
| 12 | CV2-07 0 / 4 texts | `Section::new([], B)`; 4 texts; 1 and 3 ok | Err[CV2-07 @ `section[0]`, Between{1,3}, Count(0/4)] | error |
| 13 | CV2-08 label boundaries | label `"a"*80` ok; `"a"*81`; `😀*40` ok; `😀*41` (82); `""` ok | Err[CV2-08 @ `row[0].button[0].label`, AtMost(80), Count(81/82)] | error |
| 14 | Path precision | `M([Container[T, T, Section([T], B.label("a"*81))]])` | Err path exactly `container[0].section[2].accessory.label`; Display `[CV2-08] container[0].section[2].accessory.label: limit at most 80, actual 81` | error |
| 15 | CV2-08 link url | `LinkButton::new("")`, `"h"*513`; `"h"*512` ok | Err[CV2-08 @ `…button[0].url`, Between{1,512}, Count(0/513)] | error |
| 16 | CV2-08 premium | `PremiumButton` in row | Ok (no label or emoji representable) | none |
| 17 | CV2-09 placeholder | `"a"*150` ok; `"a"*151` | Err[CV2-09 @ `row[0].select[0].placeholder`, AtMost(150), Count(151)] | error |
| 18 | CV2-09 options count | `string_select([])`; 26 options; 1 and 25 ok | Err[CV2-09 @ `row[0].select[0].options`, Between{1,25}, Count(0/26)] | error |
| 19 | CV2-09 option fields | label `""`, label/value `"a"*101`, description `"a"*101`; 100 ok | Err `…options[k].label` Between{1,100}; `.value`; `.description` AtMost(100) | error |
| 20 | CV2-09 min/max | `min_values(26)`, `max_values(0)`, `max_values(26)`, `min 3 max 2` | Err `.min_values` Between{0,25}; `.max_values` Between{1,25}; order: path `row[0].select[0]`, `Describe("min_values ≤ max_values")`, `Text("3 > 2")` | error |
| 21 | CV2-09 defaults | `user_select().max_values(2).default_users([1,2,3])`; 2 ok; String Select with 2 default options, max 1 | Err `….default_values` Between{1,2} Count(3); Between{1,1} Count(2) | error |
| 22 | CV2-10 gallery count | `MediaGallery::new([])`, 11 items; 1 and 10 ok | Err[CV2-10 @ `gallery[0]`, Between{1,10}, Count(0/11)] | error |
| 23 | CV2-10 item alt | alt `"a"*1025`; 1024 ok | Err `gallery[0].item[0].description` AtMost(1024) | error |
| 24 | CV2-11 thumbnail alt | Thumbnail alt `"a"*1025` | Err `section[0].accessory.description` AtMost(1024) | error |
| 25 | CV2-17 media url | `Media::url("")`, `"h"*2049`; 2048 ok | Err CV2-17 `….url` Between{1,2048} | error |
| 26 | CV2-13 accent | `Rgb::new(255,255,255)` accent | Ok (CV2-13 cannot fail for typed trees; lowered as `0xFFFFFF`) | none |
| 27 | CV2-05 typed | any typed tree | never yields CV2-05 (by construction) | none |
| 28 | CID-02 duplicate | `ActionRow::buttons([B(1), B(1)])` (resolved) | Err[CID-02 @ `row[0].button[1].custom_id`, Describe("a custom_id unique within the message"), Text(id)], note `first used at row[0].button[0].custom_id` | error, structural |
| 29 | CID-02 across depth | `B(1)` as Section accessory and in a Container row | one CID-02 at the later pre-order path | error |
| 30 | CID-02 triple | three `B(1)` | two violations (2nd and 3rd), each noting the first path | error |
| 31 | CID-02 distinct | `B(1)`, `B(2)` | Ok | none |
| 32 | CID-02 unresolved | two unresolved `select` Route selects with equal payloads | not compared (skipped); Ok from validate; `serde_json::to_value(body)` still errs | none |
| 33 | CV2-14 ids | 40-component tree | serialized ids are `1..=40`, unique; no runtime check exists (by construction) | none |
| 34 | ATT-05 file ok | `File::new(set.add(a))`, message `.attachments(set)` | Ok | none |
| 35 | ATT-05 other set | `File` from a ref of another `AttachmentSet` with different bytes | Err[ATT-05 @ `file[0].url`, Describe("attachment://<name> of a file in this message"), Text(url)] | error |
| 36 | ATT-05 media url | `Media::url("attachment://x.png")` in Thumbnail, no such file; `"attachment://"` | Err ATT-05 `section[0].accessory.url`; empty name also ATT-05 | error |
| 37 | ATT-05 non-ref | `Media::url("https://x.y/a.png")`, `"ATTACHMENT://a"` | no ATT-05 | none |
| 38 | ATT-05 same bytes | ref from another set whose bytes equal a file in this set | Ok (content-addressed name matches) | none |
| 39 | ATT-06 alt | `Attachment::alt("a"*1025)` in the set; 1024 ok | Err[ATT-06 @ `attachments[0].description`, AtMost(1024), Count(1025)], note ``file `<name>` `` | error |
| 40 | MSG-01 legacy | content `"a"*2000` ok; 2001; `😀*1001` | Err[MSG-01 @ `content`, AtMost(2000), Count(2001/2002)] | error |
| 41 | MSG-04 | `Users::only(1..=100)` ok; 101 users; 101 roles | Err[MSG-04 @ `allowed_mentions.users`, AtMost(100), Count(101)]; roles path `.roles` | error |
| 42 | MSG-09 V2 empty | `M(Components::new())` | Err[MSG-09 @ `components`, AtLeast(1), Count(0)] | error |
| 43 | MSG-09 legacy | `LegacyMessage::new("")`, `"  \n"` with 0 rows | Err[MSG-09 @ `content`, Describe("non-empty content or at least one Action Row"), Text(content)]; `""` with 1 row ok; `"x"` ok | error |
| 44 | Legacy rows | 5 rows ok; `ActionRow::buttons([])` row | Err CV2-06 `row[0]`; `limits.legacy_action_rows = 2` with 3 rows gives CV2-15 at `components` | error |
| 45 | Many violations | 41 texts plus a duplicate id plus a 101-user mention | every violation in the stated order; none dropped | error |
| 46 | Strict untouched | any failing or passing message under `L` | JSON of `create_body` byte-identical before and after `validate` | none |
| 47 | Lenient label | `Len`, label `"a"*81` | Ok; label becomes `"a"*79 + "…"` (80 units); warning `Truncated(Violation[CV2-08 @ path, AtMost(80), Count(81)])` | warning |
| 48 | Lenient fields | `Len`: placeholder 151, option label 101, option description 101, gallery alt 1025, thumbnail alt 1025, attachment alt 1025, legacy content 2001 | each truncated to its limit with `…`; one warning each, in pre-order (attachments, then content last) | warning |
| 49 | Lenient emoji cut | `Len`, label `😀*41` | label becomes `😀*39 + "…"` (79 units; no split surrogate) | warning |
| 50 | Lenient CV2-04 | `Len`, `T("a"*3990)` + 20× `T("bbbbbbbbbb")` | text0 → `"a"*3979+"…"` (3980), every later text → `"…"`; total 4000; Ok; one CV2-04 warning (path `components`, Count(4190)) with one `truncated <path>` note per changed text | warning |
| 51 | Lenient CV2-04 infeasible | `Len`, `limits.text_total_len = 3`, four `T("ab")` | tree unchanged for CV2-04; Err CV2-04 Count(8) | error |
| 52 | Lenient structural | `Len` + rows 3, 4, 11, 28, 41, 42, 15, 25, 19 (value) | still Err with the same violation | error |
| 53 | Lenient partial | `Len`, label 81 plus 41 components | label truncated (warning logged), but Err CV2-03; message stays mutated | error |
| 54 | Lenient within limits | `Len`, valid message | no mutation, no warnings | none |
| 55 | Lenient limit 0 | `Len`, `button_label_len = 0`, label `"ab"` | label becomes `""`, warning | warning |
| 56 | Alt missing | `M([Section([T("a")], Thumbnail::new(Media::url("https://x.y/a.png"))), MediaGallery::new([MediaItem::new(Media::url("https://x.y/b.png")).alt("  ")])])` (alt `""` likewise) | Ok with `warnings == [MissingAlt{path:"section[0].accessory"}, MissingAlt{path:"gallery[1].item[0]"}]` in both modes; `c` has the same warnings | warning |
| 57 | Alt rendered | Thumbnail from a `Rendered` `AttachmentRef`, no alt | no warning (9.1 territory) | none |
| 58 | Alt plain ref | Media from a `Plain` `AttachmentRef`, no alt | MissingAlt warning | warning |
| 59 | Logging | `validate` producing n warnings under a capturing `tracing` subscriber | exactly n WARN events, target `noctis::ui`, each with fields `rule` (empty for MissingAlt) and `path`; `cost` emits none | none |
| 60 | Cost legacy | `LegacyMessage::new("héllo").rows([row(2 buttons)])` | `components 3, text_chars 5, warnings []` | none |
| 61 | Cost no mutation | `cost` under `Len` on an over-long tree | message unchanged; no truncation warnings, only MissingAlt | none |
| 62 | Cost unresolved | unresolved select ids | `cost` works; `validate` works | none |
| 63 | Display | `UiError::Limits` with two violations | each violation's `Display`, joined by `\n` | n/a |
| 64 | Warning Display | `Truncated(v)` / `MissingAlt{path}` | `truncated to fit: {v}` / `{path}: image has no alt text` | n/a |
| 65 | MSG-09 rules fns | `validate_v2_not_empty(0, &L)`, `(1, &L)`; `validate_legacy_not_empty("", 0, &L)`, `(" ", 0)`, `("", 1)`, `("x", 0)` | 1, 0; 1, 1, 0, 0 violations | n/a |

</intent-contract>

## Code Map

- `crates/noctis-rules/src/component.rs` -- read-only reference. Descriptors are `Component`, `Container`, `Section`, `Accessory`, `Media`, `Separator`, `Spacing`, `Button`, `ButtonStyle`, `Select`, `SelectKind`, `SelectOption`, plus `Cost`. Entry points: `cost` L568, `validate_components` L610 (CV2-03 notes, CV2-04, pre-order) and `validate_legacy_components` L667. The path scheme is in the module docs L11-21. The descriptors are borrowed slices (`&'a [Component<'a>]`).
- `crates/noctis-rules/src/attachment.rs` -- `AttachmentDecl`, `validate_attachment_ref` L172 (ATT-05, empty path) and `validate_attachment_description` L146 (ATT-06, empty path). Reuse both and set the path.
- `crates/noctis-rules/src/message.rs` -- `validate_content` L99 (MSG-01, path `content`) and `validate_mentions` L113 with `AllowedMentions`/`MentionIds` over `&[u64]` (MSG-04). Add the MSG-09 functions here.
- `crates/noctis-rules/src/rule.rs` -- the `rule_ids!` list (L68: add `MSG_09 = "MSG-09"` after `MSG_08`). Counts 125→126 at L306/L308. `Violation` has fields `rule`, `path`, `limit`, `actual`, `notes`, `help`, plus `with_note`.
- `crates/noctis-rules/src/limits.rs` -- `Limits::truncates(rule)` and the `LimitMode` docs (they already promise "truncate … and log a warning").
- `crates/noctis-rules/src/len.rs` -- `discord_len`, `truncate(&str, max) -> Cow<str>`.
- `crates/noctis-rules/tests/catalog.rs` L48 and `xtask/src/rules_catalog/catalog.rs` L561 -- row count 125 → 126.
- `crates/noctis-ui/src/` (private fields; the new module is in-crate):
  - `component/layout.rs`: `Container{children, accent, spoiler}`, `ContainerComponent(ChildNode)`, `TopLevelComponent(TopNode)`, `Section{texts, accessory: AccessoryNode}`, `ActionRow(RowNode::{Buttons(Vec<Button>), Select(SelectNode)})`.
  - `component/button.rs`: `Button(ButtonNode::{Route(RouteButtonData), Link, Premium})`.
  - `component/select.rs`: `SelectNode`, `Common{custom_id, placeholder, min_values: Option<u8>, max_values, disabled}`, `defaults: Vec<DefaultValue>`, `StringSelect.options`.
  - `component/media.rs`: `Media(Source::{Url, Attachment(AttachmentRef)})` with `as_str`/`attachment`; `Thumbnail{media, alt, spoiler}`, `MediaItem`, `MediaGallery.items`, `File.file`.
  - `component/text.rs`: `TextDisplay.content`, `Separator{divider, spacing}`.
  - `attachment.rs`: `Attachment.alt`, `AttachmentSet.files` (BTreeMap), `AttachmentRef::{name, url, origin}`, `AttachmentOrigin::Rendered`.
  - `route.rs`: `CustomId::get() -> Option<&str>`.
  - `message/mod.rs`: `V2Message{components, attachments, mentions}`, `LegacyMessage{content, rows, mentions}`.
  - `message/mentions.rs`: `Users`/`Roles::Only(Vec<Id>)`.
  - `components.rs`: `Components.items`.
  - `error.rs`: `UiError`.
  - `placement.rs`: the `sealed` pattern to copy.
  - Add `pub(crate)` accessors or `pub(crate)` fields where the new module needs them.
- `crates/noctis-ui/tests/components.rs` -- the `route!` macro, `Handled` impls, `resolve_selects` and `media_ref` helpers. Copy them into the new test file.
- `crates/noctis/src/lib.rs` L297-425 -- facade `pub mod ui` (docs and `pub use`). L426-525 -- `pub mod rules` docs ("message: … MSG-01, MSG-03..06").
- `crates/noctis/tests/boundary.rs`, `tests/boundary/{msg,cid}.rs` -- the boundary module pattern.
- `docs/discord-rules.md`:
  - CID-02 L175, CV2-03 L191, CV2-04 L192, CV2-14 L202, MSG-01 L225, MSG-04 L228: How text.
  - The new MSG-09 row goes after MSG-08 (L232).
  - The "Unconfirmed values" table from L576.
- `xtask/rules-pending.toml` -- no 4.3 entries exist. CID-02/CV2-03/… diagnosed entries stay for 4.4, and MSG-09 needs none.
- `examples/project-template/.claude/skills/noctis/`:
  - `references/components-v2.md`: §0 L43 "[planned, Story 4.3]", L395 and L461 "[planned]", §15 L509-620.
  - `references/limits-cheatsheet.md`: L118 message row, L121 `RuleId::ALL (125` and the Messages table L268+.
  - `references/views-and-widgets.md`: §9 L158-161.
  - `SKILL.md`: L21 API hash.
- `examples/project-template/AGENTS.md` L38 -- the "pre-checked with" bullet.

## Tasks & Acceptance

**Execution:**
1. `Cargo.toml` (root) -- change the `tracing` entry to defaults-off with `std`, and fix the stale comment. `crates/noctis-ui/Cargo.toml` -- add `noctis-rules.workspace = true` and `tracing.workspace = true`.
2. `crates/noctis-rules/src/rule.rs`, `tests/catalog.rs`, `xtask/src/rules_catalog/catalog.rs` -- add `MSG_09` and bump the counts to 126.
3. `crates/noctis-rules/src/message.rs` -- add both functions with doctests, update the module docs, and add unit tests named `msg_09_*` (Matrix 65). Update the `lib.rs` contents line to "MSG-01, MSG-03..06, MSG-09, WH-04".
   - `pub fn validate_v2_not_empty(components: usize, limits: &Limits) -> Vec<Violation>`: path `components`, `AtLeast(1)`, `Count(0)`.
   - `pub fn validate_legacy_not_empty(content: &str, rows: usize, limits: &Limits) -> Vec<Violation>`: violates when `content.trim().is_empty() && rows == 0`. Path `content`, `Describe("non-empty content or at least one Action Row")`, `Text(content)`.
4. `docs/discord-rules.md`:
   - Add the row `| **MSG-09** Non-empty message | A message needs at least one of content, embeds, sticker_ids, components, files or poll **[D]**; a V2 message needs at least one component **[C]** | [MSG §Create Message](https://discord.com/developers/docs/resources/message#create-message) | Runtime | `noctis_ui::validate`: a V2 message with no top-level component, or a legacy message whose content is empty or whitespace-only and that has no Action Row, is rejected before sending. |`.
   - Add an Unconfirmed-values row for MSG-09 (whitespace-only content treated as empty; V2 needs ≥ 1 component).
   - Change the How text of CID-02, CV2-03, CV2-04, MSG-01 and MSG-04 to name `noctis_ui::validate`. CV2-14 How: "no runtime check: ids are assigned in pre-order, so duplicates cannot be built".
   - Leave the Enforcement columns unchanged.
5. `crates/noctis-ui/src/limits/` (`mod.rs`, `lower.rs`, `walk.rs`, `lenient.rs`; layout is the implementer's choice):
   - `pub trait Validate: sealed::Sealed {}`, implemented for `V2Message` and `LegacyMessage`.
   - `pub fn validate<M: Validate>(message: &mut M, limits: &Limits) -> Result<CostReport, UiError>`, `#[must_use]` via `Result`.
   - `pub fn cost<M: Validate>(message: &M, limits: &Limits) -> CostReport`.
   - `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub struct CostReport { pub components: usize, pub text_chars: usize, pub warnings: Vec<LimitWarning> }`. V2: rules `cost`. Legacy: the rules `cost` of the lowered rows, and `text_chars = discord_len(content)`.
   - `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub enum LimitWarning { Truncated(Violation), MissingAlt { path: String } }`, with `fn path(&self) -> &str`, `fn rule(&self) -> Option<RuleId>` and `Display` per Matrix 64.
   - Order of work in `validate`:
     1. If Lenient, truncate (per-field in pre-order, then CV2-04, then ATT-06, then MSG-01).
     2. Lower and run every check per the Boundaries order.
     3. Collect MissingAlt in pre-order.
     4. `tracing::warn!(target: "noctis::ui", rule = …, path = …, "{warning}")` once per warning.
     5. Return `Ok(report)` or `Err(UiError::Limits { violations })`.
   - Unresolved `CustomId`s are skipped by CID-02.
   - Rustdoc for each item, with runnable examples showing a path-precise error and a lenient truncation.
6. `crates/noctis-ui/src/error.rs` -- add `Limits { violations: Vec<Violation> }` (never empty). Display: the violations' `Display` joined by `"\n"`.
7. `crates/noctis-ui/src/lib.rs` -- `mod limits;`. Re-export `validate, cost, Validate, CostReport, LimitWarning`. Add a crate-doc section "# Limits" with a doctest.
8. `crates/noctis-ui/tests/validate.rs` -- one test (or a small table-driven group) per Matrix row 1-64, named with the rule id snake token (`cv2_03_…`, `cid_02_…`, `att_05_…`, `msg_09_…`, `cv2_04_lenient_…`). Include `assert_send_sync` for `CostReport` and `LimitWarning`. Row 59 uses a hand-written `tracing::Subscriber` (record level, target and field names via a `Visit`) installed with `tracing::subscriber::with_default`; no new dev-dependency.
9. `crates/noctis/src/lib.rs` -- `pub mod ui`:
   - Add the re-exports and a "Limits" doc group (validate/cost/CostReport/LimitWarning, Strict vs Lenient, path format, `UiError::Limits`).
   - Add a doctest using `noctis::rules::Limits`.
   - Update the `rules` module doc message line to include MSG-09.
10. `crates/noctis/tests/validate.rs` -- use only `noctis::ui`/`noctis::rules`. A derived `#[route]` Route with `Handled`. Matrix 14 (path and Display), 28 and 47 through the facade.
11. `crates/noctis/tests/boundary.rs` + new `tests/boundary/cv2.rs` -- runtime boundary passes via `noctis::ui::validate`: exactly 5 buttons (`cv2_06_…`), 40 components (`cv2_03_…`), 4000 text characters (`cv2_04_…`), an 80-character label (`cv2_08_…`), 25 options (`cv2_09_…`), 10 gallery items (`cv2_10_…`).
12. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`. Write the hash into `SKILL.md` L21.
13. `examples/project-template/.claude/skills/noctis/references/components-v2.md`:
    - Remove the "[planned]" markers for the runtime validator (§0, legacy content, MSG-04).
    - Add a "## 16. Runtime validation: validate and cost" section (no builder name in backticks in the heading) covering:
      - the signatures and `Validate` (V2Message, LegacyMessage);
      - every check with its path, and the violation order;
      - Strict/Lenient behavior and the exact truncatable fields;
      - the CV2-04 lenient algorithm;
      - `CostReport` fields and `LimitWarning::{Truncated, MissingAlt}`;
      - `UiError::Limits { violations }`;
      - logging target `noctis::ui`;
      - "the Responder runs validate before every send [planned]".
    - §15: list `validate_v2_not_empty`/`validate_legacy_not_empty`.
14. `examples/project-template/.claude/skills/noctis/references/limits-cheatsheet.md`:
    - Add the MSG-09 row.
    - Change `RuleId::ALL (126`.
    - Add the two functions to the `message::{…}` row.
    - `views-and-widgets.md` §9: add "for a built message today: `noctis::ui::cost(&msg, &limits)` → `CostReport { components, text_chars, warnings }`".
15. `examples/project-template/AGENTS.md` L38 -- add "whole messages with `noctis::ui::validate(&mut msg, &limits)` / `noctis::ui::cost(&msg, &limits)`". Template `src/` is unchanged: no template code calls the validator, because the Responder will.

**Acceptance Criteria:**
- Given a runtime-built `V2Message` that breaks any limit in the Matrix, when `noctis::ui::validate(&mut msg, &limits)` runs, then it returns `Err(UiError::Limits { violations })` naming each violation's path, rule id, bound and actual value, and nothing is mutated in Strict mode.
- Given `Limits` with `mode = Lenient`, when display text is over its limit, then it is truncated with `…`, a `LimitWarning::Truncated` is returned and logged at WARN, and structural rules (CV2-03, CV2-05, CV2-06, CID-01, CID-02) still fail.
- Given a non-`Rendered` Thumbnail or gallery item without alt text, when `validate` or `cost` runs in Strict mode, then a `MissingAlt` warning names its path in the `CostReport`.
- Given any message, when `noctis::ui::cost(&msg, &limits)` is called, then it returns component count, text characters and warnings without mutating or sending.
- Given the catalog change, when `cargo run --release -p xtask -- rules-catalog` runs, then it passes with MSG-09 covered by a `msg_09` unit test, and `RuleId::ALL` matches the catalog (126 rows).
- Given the facade, when a bot crate uses only `noctis::ui` and `noctis::rules`, then it validates messages (facade test), and `api-snapshot` and `skill-check` pass.

## Spec Change Log

## Review Triage Log

## Design Notes

- **`&mut` instead of `&tree`:** Lenient mode must truncate in place, and core's Responder already holds the message mutably (it resolves `custom_ids_mut` before sending). So `validate` takes `&mut M`, and Strict mode never writes. The epic's `validate(&tree, &limits)` / `cost(&tree)` are notation. `cost` keeps `&Limits` because AD-3 says every measuring function takes it.
- **Lowering without arenas or unsafe:** the descriptors borrow nested slices, so lower bottom-up in separate local `Vec`s, one per nesting level, each built by its own pre-order walk with a cursor:
  1. Leaves: Section text lists, String Select option lists and gallery media lists.
  2. Action Row item lists, which borrow the option lists.
  3. Container child lists.
  4. The top level.

  The depth is fixed by the types, so four levels are enough. Generate the CID-02/ATT-05/MissingAlt paths with the same kind names, and cross-check this in a test (the same node gets the same path prefix from both sources).
- **Lenient CV2-04 algorithm** (deterministic):
  - Let `n` be the number of Text Displays in pre-order and `L` the limit.
  - Walk the texts in order. For each, the allowance is `a = L − used − (n − j − 1)`.
  - Keep the text while its length is at most `a`.
  - The first text over its allowance becomes `truncate(text, a)`, and every later text becomes `truncate(text, 1)` (`"…"`, or itself if ≤ 1 unit).
  - This always fits when `L ≥ n`. When `L < n` the tree is left unchanged and CV2-04 fails.
- **Warnings in both modes:** the AC names Strict mode for missing alt, but Story 9.1 says "any other image without alt text keeps Story 4.3's warning" in both modes, so the warning does not depend on the mode. Only the component's own `alt` counts (Discord shows the component `description`). An empty or whitespace-only `alt` counts as missing.
- **MSG-09 is new:** the 4.2 review deferred "empty bodies serialize though Discord 400s" to this story. Adding a catalog rule follows the AGENTS.md rule (catalog + `noctis-rules`, same id).
- **Numeric ids (CID-02/CV2-14):** these are unique by construction (pre-order numbering), so the runtime only checks `custom_id`s.
- Sketch:
  ```rust
  let mut msg = V2Message::new(components).attachments(files);
  match noctis::ui::validate(&mut msg, &limits) {
      Ok(report) => assert!(report.components <= limits.components),
      Err(UiError::Limits { violations }) => eprintln!("{}", violations[0]), // [CV2-08] container[0]...
      Err(_) => unreachable!(),
  }
  ```

## Verification

**Commands** (repo root; CI is paused, so local gates are the whole check; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (noctis-ui `validate` tests, doctests, facade `validate`/`boundary`/`ui`, noctis-rules catalog, xtask).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0 (new lock entries: `tracing`, `tracing-core`).
- `cargo run --release -p xtask -- deps-check`, `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- exit 0 each.
- `bash .github/scripts/check-build-config.sh` -- exit 0; `test ! -d target/debug` -- true.
