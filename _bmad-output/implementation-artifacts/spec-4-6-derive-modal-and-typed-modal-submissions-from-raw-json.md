---
title: 'Story 4.6: `derive(Modal)` and typed modal submissions from raw JSON'
type: 'feature'
created: '2026-10-20'
status: 'done'
baseline_revision: 'ae7a27d5f788510fc31be4ac797ec9ddbaa7b4a3'
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

**Problem:** Story 4.5 gave Noctis a runtime-built `Modal`, but a bot author cannot declare a modal as a struct, literal modal texts and counts are not checked at `cargo check` (`xtask/rules-pending.toml` still holds diagnosed MOD-01, MOD-02, MOD-04, MOD-05, MOD-06, MOD-07 and unit MOD-09 for 4.6), and a modal submission cannot be decoded at all; twilight-model 0.17.1 turns Radio Group (21), Checkbox Group (22) and Checkbox (23) into `ModalInteractionComponent::Unknown(u8)` and drops their values (FR-29, FR-64, FR-78; AD-12).

**Approach:** Add `#[derive(Choices)]` (unit enum → option set with a type-level count) and `#[derive(Modal)]` (struct → `ModalForm` impl) to `noctis-macros`. The derive checks literals at expansion through `noctis_rules::modal::validate_modal`, builds the 4.5 `Modal` from a Route value, and decodes submissions from raw interaction JSON `&[u8]` through a new `noctis_ui::ModalSubmission` that parses Discord's JSON itself (never through twilight's modal component enum), joining select ids and File Upload ids to `data.resolved`. File Upload values are `ResolvedAttachment`s with an injectable-fetcher download helper.

## Boundaries & Constraints

**Always:**
- **Public items** (flat in `noctis_ui`, re-exported from `noctis::ui`; derives and the two traits also in `noctis::prelude`): traits `ModalForm`, `Choices`, `FileFetcher`, `RadioOptions`, `CheckboxOptions`, `StringSelectOptions`; types `ModalSubmission`, `SubmittedValue`, `SubmitError`, `ResolvedUser`, `ResolvedRole`, `ResolvedChannel`, `Mentionable`, `ResolvedAttachment`, `DownloadError`; derives `noctis::prelude::{Modal, Choices}` (macro namespace; no clash with the `noctis::ui::Modal` struct). Everything else the expansion needs is `#[doc(hidden)]` under `noctis_ui::__macro` and reached only as `::noctis::__private::ui::__macro::…` (AD-2; hidden items stay out of `xtask/public-api.txt`).
- **`ModalForm`** (exact shape):
  ```rust
  pub trait ModalForm: Sized {
      type Route: Route<Payload = Self::Route> + Handled + Send + Sync + 'static;
      fn modal(route: Self::Route) -> Modal;                       // via RouteComponents::modal
      fn from_submission(submission: &ModalSubmission) -> Result<Self, SubmitError>;
      fn from_json(bytes: &[u8]) -> Result<Self, SubmitError> {     // provided
          Self::from_submission(&ModalSubmission::from_slice(bytes)?)
      }
  }
  ```
  Decoding never checks `data.custom_id` against `Self::Route` (dispatch is core's, Story 7.8); it is exposed as `ModalSubmission::custom_id()`.
- **`Choices`** (exact shape): `pub trait Choices: Sized + 'static { type Count; const ALL: &'static [Self]; fn label(&self) -> &'static str; fn value(&self) -> &'static str; fn description(&self) -> Option<&'static str>; fn from_value(value: &str) -> Option<Self>; }`. `Count` is `Arity<N>` with `N` = variant count (doc it as "type-level count; always `Arity<N>`"). `RadioOptions` (2..=10, message `[MOD-07] a Radio Group takes 2 to 10 options`), `CheckboxOptions` (1..=10, `[MOD-07] a Checkbox Group takes 1 to 10 options`) and `StringSelectOptions` (1..=25, `[CV2-09] a String Select takes 1 to 25 options`) are sealed `Arity` bounds in `arity.rs`, styled like `RowButtons`, each note naming `{Self}` as the variant count and pointing at the runtime builders (`RadioGroup::new` + `validate`); extend `arity!(sealed: …)` to 0..=25.
- **`#[derive(Choices)]`** on a non-generic enum of unit variants only. Variant attribute `#[choice("Label")]` or `#[choice("Label", value = "v", description = "d")]` (label required; `value` defaults to the variant ident as written, raw prefix stripped). Expansion checks with `discord_len` against `Limits::default().modal_option_text_len`: label and value 1..=100, description ≤ 100, error `[MOD-07]`; zero variants, a non-unit variant, generics or a non-enum item are `[MOD-07]` errors; a duplicate value is a plain error `duplicate choice value \`v\`: every variant needs its own value` (no rule id: it is a decode requirement, not a Discord rule). Malformed attribute syntax is a plain `syn` error.
- **`#[derive(Modal)]`** on a non-generic struct with named fields. Item attribute `#[modal(route = Path, title = "literal")]`, both required. One field = one top-level component, in declaration order; each field carries exactly one of these attributes (first positional literal = Label text, MOD-04; every Label attribute also takes `description = "…"`):

  | Attribute | Extra args | Field types (hidden sealed trait per kind, `#[diagnostic::on_unimplemented]` naming the kind and the allowed types) |
  |---|---|---|
  | `text_input("L", …)` | `style = short\|paragraph` (default short), `min_length`, `max_length`, `placeholder` | `String`, `Option<String>` |
  | `string_select("L", …)` | `placeholder`, `min_values`, `max_values` | `C`, `Option<C>`, `Vec<C>` (`C: Choices`) |
  | `user_select` / `role_select` / `mentionable_select` / `channel_select("L", …)` | `placeholder`, `min_values`, `max_values`; channel also `channel_types = [GuildText, …]` (twilight `ChannelType` variant idents) | `ResolvedUser` / `ResolvedRole` / `Mentionable` / `ResolvedChannel`, each also `Option<_>`, `Vec<_>` |
  | `file_upload("L", …)` | `min_values`, `max_values`, `file_types = ["image", ".pdf"]` | `ResolvedAttachment`, `Option<_>`, `Vec<_>` |
  | `radio_group("L")` | — | `C`, `Option<C>` |
  | `checkbox_group("L", …)` | `min_values`, `max_values` | `Vec<C>` |
  | `checkbox("L")` | — | `bool` |
  | `text_display("content")` | — (no Label) | `()` |

  Key = field name (raw `r#` stripped); it is the input's `custom_id`. `Option<_>` ⇒ `.required(false)`; plain ⇒ required (default, omitted); `Vec<_>` ⇒ required unless `min_values = 0`, which emits `.required(false)`. `min_values`/`max_values` only on `Vec` fields (else plain error). Built `max_values` for a `Vec` with none declared: String Select `C::ALL.len()`, other selects 25, File Upload 10, Checkbox Group omitted (Discord default = option count). Options come from `C::ALL` (`label`, `value`, `description`), the option-count bound is a trait obligation on `<FieldTy as hidden kind trait>::Count` that fails under `cargo check`, spanned on the field type.
- **Expansion checks** (all errors at once, each `syn::Error` spanned on the offending literal/attribute/field, text = the `Violation`'s `Display`, like `components!`): lower to `noctis_rules::modal::{Modal, Label, LabelChild, …}` with `Limits::default()` and run `validate_modal`; Choices-backed groups/selects lower with sentinel options of the maximum legal count (`"x"`: Radio 10, Checkbox Group 10, String Select 25) so only literal facts are judged; then `validate_input_custom_id` per key (CID-01) and the summed `text_display` literals against CV2-04 (`text_total_len`). Option counts versus `min_values`/`max_values` stay runtime (`noctis::ui::validate`). `title(key = …)`, `key = …` or any non-literal text is rejected with the relevant rule id (MOD-01 title, MOD-04 label) and help `localized modal text is [planned, Epic 11]; use a literal`. An unattributed field is an error citing MOD-02 and a field with two kind attributes one citing MOD-03; struct shape errors (tuple, unit, enum, generics) cite MOD-02.
- **`ModalSubmission::from_slice(bytes)`** parses with `serde_json` (`serde_json.workspace = true` added to noctis-ui `[dependencies]`; tokio `["macros","rt"]` to its dev-dependencies) the whole interaction object: `type` must be 5 (else `NotModalSubmit { kind }`), `data.custom_id: string`, `data.components: array`, optional `data.resolved` (deserialized as `twilight_model::application::interaction::InteractionDataResolved`; a per-component `resolved` object, which Discord's docs table also lists, is merged into it). Walk: type 18 → its `component`; type 1 → its `components`; type 10 → skipped; types 3,4,5,6,7,8,19,21,22,23 → keyed `SubmittedValue`; any other type (or a non-object) → kept verbatim in `unknown(): &[serde_json::Value]`, never dropped. `SubmittedValue` (`#[non_exhaustive]`, `Debug, Clone, PartialEq`): `TextInput(String)` (4 `value`), `StringSelect(Vec<String>)` (3 `values`), `UserSelect(Vec<Id<UserMarker>>)`, `RoleSelect(Vec<Id<RoleMarker>>)`, `MentionableSelect(Vec<Id<GenericMarker>>)`, `ChannelSelect(Vec<Id<ChannelMarker>>)`, `FileUpload(Vec<Id<AttachmentMarker>>)` (5,6,7,8,19 `values`, snowflake strings), `RadioGroup(Option<String>)` (21 `value`, null = None), `CheckboxGroup(Vec<String>)` (22 `values`), `Checkbox(bool)` (23 `value`). Accessors: `custom_id()`, `get(key) -> Option<&SubmittedValue>`, `resolved() -> &InteractionDataResolved`, `unknown()`.
- **`SubmitError`** (`thiserror`, `Debug, Clone, PartialEq, Eq`, `#[non_exhaustive]`; Display starts `[MOD-09]`): `Malformed { reason: String }` (bad JSON, missing/ill-typed `type`/`data`/`custom_id`/`components`, a known-type component without a string `custom_id`, bad `resolved`), `NotModalSubmit { kind: u64 }`, `Duplicate { key }` (same key twice), `Missing { key }`, `WrongType { key, expected: u64, found: u64 }`, `Invalid { key, reason }` (ill-typed value, bad snowflake, unknown choice value, length/count outside the declared bounds), `Unresolved { key, id: u64 }`. Never a panic, `unwrap`, `expect` or slice index on input data.
- **Derived decode per field** (key lookup in `ModalSubmission`): absent component ⇒ `Option` → `None`, `Vec` → `[]` if not required else `Missing`, plain → `Missing`; a different `SubmittedValue` variant (or the key in `unknown`) ⇒ `WrongType` (expected = the attribute's Discord type number; found = the submitted `type`). Text: `""` ⇒ `None` for `Option<String>`, `Missing` for `String`; `discord_len` outside the declared `min_length..=max_length` (or > 4000) ⇒ `Invalid`. Single-valued select/upload/radio: 0 values ⇒ None/`Missing`, > 1 ⇒ `Invalid`. `Vec`: count outside effective `min..=max` (min = declared, else 1 if required else 0; max = the built max, Checkbox Group default `C::ALL.len()`) ⇒ `Invalid`. Choice values via `C::from_value` (else `Invalid`). Users: `resolved.users[id]` required, `member` = `resolved.members.get(id)`; Mentionable: user first, then role; roles/channels/attachments from their maps; absent ⇒ `Unresolved`. `checkbox` absent ⇒ `Missing`. `text_display` ⇒ `()` and ignores the submission.
- **Resolved types** (`Debug, Clone, PartialEq`, `#[non_exhaustive]`, pub fields): `ResolvedUser { user: User, member: Option<InteractionMember> }`, `ResolvedRole { role: Role }`, `ResolvedChannel { channel: InteractionChannel }`, `enum Mentionable { User(ResolvedUser), Role(ResolvedRole) }`, `ResolvedAttachment { attachment: Attachment }` (twilight-model types) with `id()`, `filename()`, `size()`, `content_type()`, `url()`. Story 6.2's options reuse these names.
- **Download helper**: `pub trait FileFetcher { fn fetch(&self, url: &str, max_bytes: u64) -> impl Future<Output = Result<Bytes, Box<dyn std::error::Error + Send + Sync>>> + Send; }` (core supplies the HTTP impl later; noctis-ui does no I/O, AD-2/AD-15). `ResolvedAttachment::download(&self, fetcher: &F, limits: &Limits) -> Result<Bytes, DownloadError>` (`async`, `F: FileFetcher + ?Sized + Sync`): URL must start with `https://cdn.discordapp.com/` or `https://media.discordapp.net/` (else `DownloadError::Host { url }`, fetcher not called); `size > limits.attachment_bytes` ⇒ `TooLarge { size, limit }` (not called); calls `fetch(url, size)`; returned length ≠ `size` ⇒ `SizeMismatch { expected, actual }`; fetch failure ⇒ `Fetch(source)` (`#[source]`). Its rustdoc and `ResolvedAttachment`'s state: the CDN URL is ephemeral (signed, expires), download promptly, re-check `content_type` (MOD-09).
- Every new public item has rustdoc with a doctest. Release-only builds/tests (AGENTS.md); trybuild needs no `--release`.

**Never:**
- No pre-fill setters or `prefill(..)` on derived modals (Story 9.4); no localized texts / `key =` (Epic 11); no Responder, `ModalSubmitCtx`, `#[handler]` or `opens_modal` work (Story 7.8); no Route-vs-`custom_id` check in decode.
- Never decode through `twilight_model::…::modal::ModalInteractionData`/`ModalInteractionComponent` (it drops types 21-23); twilight is used only for ids and `InteractionDataResolved` contents.
- No change to `noctis_rules::modal::validate_modal` semantics or paths, to 4.5 builder APIs, or to `noctis::ui::Label`.
- No fixture `.stderr` or test may cite a rule still pending for another story (no `[OPT-08]`/`[OPT-09]`, no test fn named `opt_0x`/`cmd_`); no new workspace dependency (serde_json and tokio are already workspace deps).

## I/O & Edge-Case Matrix

Default Limits. "CE R" = compile error containing `[R]` (trybuild fixture when marked F). Decode rows call `T::from_json(bytes)`; `E::X` = `SubmitError::X`.

| # | Scenario | Input | Expected |
|---|---|---|---|
| 1 | Happy derive build | form of every kind (text_display + text_input + radio, …≤5) | `T::modal(route)` JSON = insta snapshot; keys = field names; `validate` Ok |
| 2 | Field count 5 / 6 / 0 | struct | Ok / CE MOD-02 F / CE MOD-02 F |
| 3 | Title 45 / 46 / "" / `"😀"×23` | `title` | Ok / CE MOD-01 F / CE MOD-01 / CE MOD-01 (UTF-16 46) |
| 4 | Label 45 / 46 / ""; description 100 / 101 | first literal, `description` | Ok / CE MOD-04 F / CE MOD-04 ; Ok / CE MOD-04 |
| 5 | Text Input min 0 max 4000 / max 0 / max 4001 / min 10 max 5 / placeholder 100 / 101 | args | Ok / CE MOD-05 / CE MOD-05 / CE MOD-05 F / Ok / CE MOD-05 |
| 6 | File Upload max 10 / 11 / file_types 10 / 11 / `"exe"` | args | Ok / CE MOD-06 F / Ok / CE MOD-06 / CE MOD-06 |
| 7 | Radio Choices 1 / 2 / 10 / 11 variants | `C` field | CE MOD-07 F / Ok / Ok / CE MOD-07 F |
| 8 | Checkbox Group Choices 1 / 10 / 11; `min_values = 11` | `Vec<C>` | Ok / Ok / CE MOD-07 F / CE MOD-07 |
| 9 | String Select Choices 25 / 26; placeholder 150 / 151 | `C` | Ok / CE CV2-09 / Ok / CE CV2-09 |
| 10 | Choice label/value 100 / 101 / ""; description 101; 0 variants; tuple variant; duplicate value | `derive(Choices)` | Ok / CE MOD-07 / CE MOD-07 / CE MOD-07 / CE MOD-07 / CE MOD-07 / plain error |
| 11 | Key 100 / 101 chars; raw ident `r#type` | field name | Ok / CE CID-01 ; key `"type"` |
| 12 | text_display total 4000 / 4001 | literals | Ok / CE CV2-04 |
| 13 | Wrong field type / no attribute / two attributes / `min_values` on non-Vec / title non-literal or `key =` / missing `route` / tuple struct, enum or generics | | E0277 (doctest) / CE MOD-02 / CE MOD-03 / plain / CE MOD-01 (help planned Epic 11) / plain / CE MOD-02 |
| 14 | Route without `Handled` | `#[modal(route = R)]` | E0277 `[CID-04]` (compile_fail doctest) |
| 15 | Decode happy, Label-wrapped | three derived forms (each ≤ 5 fields) covering all ten input kinds plus a `text_display`, each with its fixture holding resolved users/members/roles/channels/attachments | every field populated; `ResolvedUser.member` Some when a member is present, None when only the user is |
| 16 | FR-64 drop proof | same fixture's `data` through twilight `ModalInteractionData` | twilight yields `Unknown(21/22/23)`; ours yields `Some(C)`, `vec![..]`, `true` |
| 17 | Legacy Action-Row-wrapped Text Input | `{type:1, components:[{type:4,…}]}` | decodes |
| 18 | Optional empties | text `""`, radio `null`, select `[]`, upload `[]` on `Option` fields | all `None` |
| 19 | Required empties / absent | same on plain fields; key missing | `E::Missing{key}` |
| 20 | Wrong type | key `bio` submitted as type 3 | `E::WrongType{key:"bio",expected:4,found:3}` |
| 21 | Ill-typed values | `value: 5` on 4; `values: "x"`; `value: "yes"` on 23; snowflake `"abc"` / `"0"` | `E::Invalid` each |
| 22 | Unknown choice | radio `"zzz"` | `E::Invalid` |
| 23 | Unresolved | user id not in `resolved.users`; attachment id missing; mentionable neither | `E::Unresolved{key,id}` |
| 24 | Count/length bounds | single select with 2 values; `Vec` over max / under min; text over `max_length` (unicode `discord_len`) | `E::Invalid` |
| 25 | Unknown component | `{type:99,custom_id:"x",foo:1}` and an unclaimed known key | Ok; `unknown()` holds the type-99 object verbatim; `get("x2")` holds the unclaimed value |
| 26 | Duplicate key | two components `custom_id:"bio"` | `E::Duplicate` |
| 27 | Not a modal submit / malformed | `type:3`; `b"not json"`; `b""`; `data` missing; `components` not array; component without `custom_id`; 200-deep nesting | `E::NotModalSubmit{kind:3}` / `E::Malformed` each, no panic |
| 28 | Per-component `resolved` | resolved inside the select object, not `data` | joins |
| 29 | Download ok / bad host / http / too large / size mismatch / fetch error | fake `FileFetcher` | `Bytes` / `Host` (not called) / `Host` / `TooLarge` (not called) / `SizeMismatch` / `Fetch` |
| 30 | `Vec` with `min_values = 0` | checkbox_group | JSON has `"min_values":0,"required":false`; empty submission → `vec![]` |

</intent-contract>

## Code Map

- `crates/noctis-macros/src/lib.rs` -- register `#[proc_macro_derive(Modal, attributes(modal, text_input, string_select, user_select, role_select, mentionable_select, channel_select, file_upload, radio_group, checkbox_group, checkbox, text_display))]` and `#[proc_macro_derive(Choices, attributes(choice))]`; update crate docs "Implemented so far". No helper named `label` (it would clash with the `label!` macro in the prelude glob).
- `crates/noctis-macros/src/common.rs` -- `error`, `combine`, `reject_generics` (pattern; its CID-01 text does not fit, write modal-specific variants). `literal.rs` -- literal measuring helpers. `components/lower.rs` L37-57, L304 -- pattern for lowering into `noctis_rules` descriptors and mapping a `Violation` path to a span.
- `crates/noctis-rules/src/modal.rs` -- descriptors, `validate_modal` (L478), `validate_input_custom_id` (L847); read-only. `limits.rs` L98-133 field names (`modal_option_text_len`, `text_total_len`, `attachment_bytes`). `len::discord_len`.
- `crates/noctis-ui/src/modal/{mod,input,select}.rs` -- 4.5 builders the derive calls (`ModalLabel::new`, `.description`, `TextInput::new(key, style)`, `ModalStringSelect::new(key, options)`, `ModalChannelSelect::channel_types`, `FileUpload::file_types`, `RadioGroup::new`, `CheckboxGroup::new`, `Checkbox::new`, `ModalOption::new(..).description(..)`, `SelectOption`). New files here: `form.rs` (`ModalForm`, `Choices`, hidden kind traits), `submit.rs` (`ModalSubmission`, `SubmittedValue`, `SubmitError`), `resolved.rs` (resolved types, `FileFetcher`, `DownloadError`).
- `crates/noctis-ui/src/arity.rs` -- add the three option-count bounds; `arity!` macro L120+.
- `crates/noctis-ui/src/route.rs` -- `RouteComponents::modal` and `Handled` (bound reused).
- `crates/noctis-ui/src/lib.rs` -- module list L206-217, `pub use` L219-245, hidden `__macro` L249 (add the expansion helpers and a `ChannelType` re-export), crate docs (extend "# Modals").
- `crates/noctis-ui/Cargo.toml` -- add `serde_json.workspace = true` (normal) and `tokio = { workspace = true, features = ["macros", "rt"] }` (dev). Locked serde_json 1.0.154; twilight-model =0.17.1 `InteractionDataResolved` has `attachments, channels, members, messages, roles, users` HashMaps keyed by `Id`.
- `crates/noctis/src/lib.rs` -- prelude L290-309 (add `Modal`, `Choices` derives and `ModalForm`, `Choices` traits), `__private` L314, `ui` docs and re-exports (L319-616).
- `crates/noctis/tests/` -- `ui/mod/` (existing MOD-03 fixtures; add new), `boundary.rs` + `boundary/modal.rs` (extend), `derive_hygiene.rs` (extend), new `modal_derive.rs` + `fixtures/modal/*.json`.
- `xtask/rules-pending.toml` L286-319 -- the seven `story = "4.6"` entries to remove. `xtask/src/rules_catalog/mod.rs` L1-30 evidence rules.
- `docs/discord-rules.md` L211-219 -- MOD-01..07, MOD-09 How cells.
- `examples/project-template/` -- `src/modules/profile.rs` L94-160 (`Pronouns`, `BioForm`, `edit_bio`, `save_bio`), `.claude/skills/noctis/references/modals.md` (§1, §2, §3, §4 intro, §4.8, §4.9, §4.12, §7 intro), `limits-cheatsheet.md` MOD rows, `SKILL.md` L21 API hash, `AGENTS.md` L38.
- Continuity (4.5): keys are strings serialized as `custom_id`; `RouteComponents::modal` is the only `Modal` constructor; two Radio/Checkbox default-count gaps stay deferred (not this story). Same gates and trybuild bless command as 4.5.

## Tasks & Acceptance

**Execution:**
1. `crates/noctis-ui/src/arity.rs` -- `RadioOptions`, `CheckboxOptions`, `StringSelectOptions` + sealed 0..=25, with pass and `compile_fail` doctests.
2. `crates/noctis-ui/src/modal/form.rs` -- `ModalForm`, `Choices`, hidden per-kind field traits (decode + `Count` + required/max defaults) implemented for the type table; doctests incl. E0277 for a wrong field type.
3. `crates/noctis-ui/src/modal/submit.rs` -- `ModalSubmission::from_slice`, `SubmittedValue`, `SubmitError` per Boundaries; unit tests `mod_09_*` for Matrix 17-28 at this level.
4. `crates/noctis-ui/src/modal/resolved.rs` -- resolved types, `FileFetcher`, `ResolvedAttachment::download`, `DownloadError`; tests `mod_09_download_*` (Matrix 29) with a fake fetcher counting calls, `#[tokio::test]`.
5. `crates/noctis-ui/src/{lib.rs,modal/mod.rs}`, `Cargo.toml` -- modules, re-exports, `__macro` helpers, deps, crate docs.
6. `crates/noctis-macros/src/choices.rs` + `modal.rs` (new), `lib.rs` -- the two derives per Boundaries; unit tests of the attribute parser and path→span mapping.
7. `crates/noctis/src/lib.rs` -- prelude and `ui` re-exports; doc section "Modal forms" with a facade doctest (`#[derive(Route, Modal, Choices)]`, `from_json`).
8. `crates/noctis/tests/modal_derive.rs` + `tests/fixtures/modal/{text_and_choices,entity_selects,uploads_and_checks,legacy_row,optional_empty}.json` (one realistic Discord payload each, `"type": 5`) -- Matrix 1, 15-30 end to end; test names contain `mod_09` (decode) or the rule id; Matrix 1 insta snapshot `snapshots/modal_derive__form.snap` (insta is a noctis dev-dep only if absent: add `insta = { workspace = true, features = ["json"] }`).
9. `crates/noctis/tests/ui/mod/` -- fixtures with blessed `.stderr`: `MOD-01_title_too_long`, `MOD-02_six_fields`, `MOD-02_no_fields`, `MOD-04_label_too_long`, `MOD-05_min_over_max`, `MOD-06_file_upload_max_values`, `MOD-07_radio_one_choice`, `MOD-07_radio_eleven_choices`, `MOD-07_checkbox_group_eleven_choices`. Each `.stderr` contains its `[ID]`.
10. `crates/noctis/tests/boundary/modal.rs` -- derive boundary passes: 5 fields, 45-char title/Label, 100-char description, Text Input min 0/max 4000, File Upload max 10 + 10 file types, Radio 2 and 10, Checkbox Group 1 and 10, String Select 25, 100-char choice label/value, 100-char key; names `mod_0x_derive_…`/`cid_01_…`/`cv2_09_…`.
11. `crates/noctis/tests/derive_hygiene.rs` -- add a `Choices` enum and a `Modal` struct inside `shadowed` (also shadow `Vec`, `String`, `bool`) and assert they work.
12. `xtask/rules-pending.toml` -- delete the seven 4.6 entries.
13. `docs/discord-rules.md` -- How cells only: MOD-01/02/04/05/06 name `derive(Modal)` expansion checks; MOD-07 `derive(Choices)` + `RadioOptions`/`CheckboxOptions`; MOD-09 `ModalSubmission`/`ModalForm::from_json` typed errors and `ResolvedAttachment::download`.
14. `xtask/public-api.txt` -- `cargo run --release -p xtask -- api-snapshot --bless`; put the printed hash on `SKILL.md` L21.
15. `examples/project-template/` -- `profile.rs`: real `Choices`/`Modal` syntax with literal texts (comment `// localized texts: [planned, Epic 11]`), `BioForm::modal(..)` with pre-fill marked `[planned, Story 9.4]`; `modals.md`: replace every "[planned, Story 4.6]" with the real derive syntax, the attribute/type table, decode semantics and errors, `ModalSubmission`, resolved types and the download helper (ephemeral URL); `limits-cheatsheet.md` and `AGENTS.md` L38 as needed. Run `skill-check`.

**Acceptance Criteria:**
- Given `#[derive(Modal)]` with a Route and a literal title, when `cargo check` runs, then each field is one top-level component, and the Matrix CE rows fail with their rule ids; task 9's fixtures and `rules-catalog` prove MOD-01, 02, 04, 05, 06, 07 diagnosed.
- Given a raw modal-submit interaction as `&[u8]`, when `T::from_json` runs, then every field is populated from Label-wrapped or Action-Row-wrapped components, selects join resolved users/members/roles/channels, uploads join attachments, Radio/Checkbox Group/Checkbox decode though twilight 0.17.1 drops them, unknown components stay in `ModalSubmission::unknown()` as `serde_json::Value`, and every malformed or missing input is a `SubmitError` (never a panic).
- Given `ResolvedAttachment`, when its docs are read, then they state the URL is ephemeral, and `download` enforces host, size and length.
- Given the new public API, when `api-snapshot`, `skill-check` and `rules-catalog` run, then all pass, no `story = "4.6"` entry remains, and the template's bio modal uses the real derive syntax.

## Spec Change Log

## Review Triage Log

### 2026-10-20 — Review pass
- verdicts: 34 findings — high 0, medium 0, low 21, false 13, maybe-false 0
- findings:
  - `[low]` `[patch]` (blind) `download` passes a truncated file when the fetcher stops at `max_bytes`; redirects off-CDN not forbidden — `FileFetcher` says only "should stop reading"; fix: make "error, never truncate, no off-CDN redirects" a hard documented contract (grouped with edge #10).
  - `[false]` `[reject]` (blind) declared `min_values`/`max_values` never checked against the real `Choices` count — the intent-contract assigns this explicitly to runtime ("Option counts versus `min_values`/`max_values` stay runtime (`noctis::ui::validate`)") and modals.md L118 says so; no doc overclaims it.
  - `[low]` `[patch]` (blind) an empty required `Vec` gives `Invalid`, but `SubmitError::Missing` docs and modals.md L187 say empty ⇒ `Missing` — code follows the spec's count rule; fix: reword both docs.
  - `[low]` `[reject]` (blind) duplicate submitted values accepted in `Vec` fields — Discord never sends duplicates; only a forged payload hits it, and the fix adds a guard the spec does not ask for.
  - `[low]` `[reject]` (blind) `is_vec` is syntactic, so a `Vec` alias rejects `min_values` — failure is a loud compile error; runtime build uses `MULTI` so the built modal is right; rare, fix adds machinery.
  - `[low]` `[reject]` (blind) repeated attribute arguments last-win in `derive(Modal)` — real but rare typo case; fix adds seen-name guards.
  - `[false]` `[reject]` (blind) template calls nonexistent `.bio()/.pronouns()` pre-fill setters — the spec mandates `BioForm::modal(..)` with pre-fill marked `[planned, Story 9.4]`; the template is outside the workspace and the marker is the project's planned-API convention.
  - `[low]` `[patch]` (blind) compile-fail coverage missing for new diagnostics — MOD-03 two attributes, CID-01, CV2-04, placeholders and duplicate choice values are covered by noctis-macros unit tests (modal.rs mod_02/mod_04/mod_05_to_07, choices.rs duplicate_values_and_raw_idents); only the `StringSelectOptions` 26-Choices derive path is unproven (grouped with vgap #1).
  - `[low]` `[patch]` (blind) `spans_fall_back_to_the_longest_prefix` discards its fallback lookups — confirmed at modal.rs L1025-1026; fix: assert the resolved key.
  - `[false]` `[reject]` (blind) `walk` accepts nested wrappers and relies on serde's recursion limit — nesting only yields keyed values a flat forged payload could equally carry; serde_json's limit returns an error mapped to `Malformed` (200-deep test), no panic.
  - `[low]` `[reject]` (blind) `channel_types` accepts any path prefix / unknown variants fail late — unknown variant is a loud resolution error; wrong prefixes are harmless; rare, fix adds validation branches.
  - `[low]` `[reject]` (blind) `#[cfg]`-gated `Choices` variants break the expansion — loud compile error, rare usage; fix adds a branch.
  - `[false]` `[reject]` (edge) string_select `min_values` > variant count passes `cargo check` — same intent-contract runtime assignment as the blind min/max finding.
  - `[false]` `[reject]` (edge) checkbox_group `min`/`max` > variant count passes expansion — same as above.
  - `[low]` `[reject]` (edge) same named argument twice silently last-wins/concatenates — same as blind repeated-args finding; rare, guard fix.
  - `[low]` `[reject]` (edge) `route`/`title` given twice silently overrides — rare, guard fix.
  - `[low]` `[reject]` (edge) `channel_types` prefixed paths rewritten to `ChannelType::<last>` — harmless rewrite, rare.
  - `[low]` `[reject]` (edge) `Vec` alias vs syntactic `is_vec` — same as blind; loud compile error.
  - `[low]` `[reject]` (edge) repeated values decode into `Vec<C>` with duplicates — same as blind; forged-only.
  - `[low]` `[reject]` (edge) resolved channel kind / attachment type not checked against declared `channel_types`/`file_types` — Discord enforces these client-side for signed interactions; the spec does not ask for it and the fix adds parameters to decode.
  - `[low]` `[reject]` (edge) unknown-type component sharing a known key not reported as `Duplicate` (submit.rs L332-339) — forged-only, known value wins deterministically; fix adds a guard.
  - `[low]` `[patch]` (edge) a fetcher ignoring `max_bytes` buffers an oversized body before `SizeMismatch` — grouped with the blind truncation finding; documented hard contract.
  - `[low]` `[patch]` (vgap) no compile_fail fixture for a 26-Choices `#[string_select]` (`StringSelectOptions`) — pre-verified; add `CV2-09_derive_string_select_26_choices` trybuild fixture.
  - `[low]` `[patch]` (vgap) File Upload default `max_values` 10 for `Vec` without `max_values` never observed — pre-verified; add a derived field, JSON and 10/11 decode asserts in modal_derive.rs.
  - `[low]` `[patch]` (vgap) download tests never check the URL / `max_bytes` passed to the fetcher — pre-verified; fake records args, assert in `mod_09_download_ok`.
  - `[low]` `[patch]` (vgap) implicit choice value from a > 100-char variant ident untested — pre-verified; add case to `mod_07_choice_texts`.
  - `[false]` `[reject]` (vgap other) min/max versus real `Choices` count unchecked at expansion — intent-contract keeps it runtime; modals.md L118 documents it.
  - `[false]` `[reject]` (intent D1) handler-level receipt not exercised — intent-contract excludes `ModalSubmitCtx`/`#[handler]` (Story 7.8); epics L2030 hands bytes to the 4.6 decoder.
  - `[false]` `[reject]` (intent D2) FR-64 fixtures hand-authored — the spec asks for realistic payloads per Discord docs, and `mod_09_twilight_drops_what_noctis_decodes` proves twilight's loss on the same bytes.
  - `[false]` `[reject]` (intent D3) "every field populated" split over three forms — MOD-02's 5-component cap makes one struct with ten kinds impossible; Matrix 15 settles three forms.
  - `[false]` `[reject]` (intent D4) no concrete HTTP fetcher — intent-contract: noctis-ui does no I/O, core supplies the fetcher later (AD-2/AD-15); AC is at the docs surface.
  - `[false]` `[reject]` (intent D5) unknown data only on `ModalSubmission` — intent-contract specifies `unknown()` there and `WrongType` for an unknown carrying a declared key; "unknown component data" is components, not extra properties.
  - `[false]` `[reject]` (intent D6) template reconciliation textual, literal texts vs locale rule — intent-contract mandates literal texts (`key =` is Epic 11) and the `[planned, Story 9.4]` pre-fill marker; the locale entries stay for Epic 11.
  - `[false]` `[reject]` (intent D7) extra decode bounds, default-limit text cap, MOD-03 stderr path change — the intent-contract requires the decode bounds and the 4000 cap; the stderr only prints the full type path and still carries `[MOD-03]`.

## Design Notes

- **Attributes per input, Label text positional:** a `#[label]` helper would collide with the prelude's `label!` macro in the macro namespace; one attribute per field also makes "one field = one component" structural.
- **Choices-backed options only:** option counts get a compile tier through `Choices::Count`; free-form option lists stay on the runtime builder (`RadioGroup::new`).
- **Sentinel options at expansion:** max legal counts never trigger count/min/max rules, so the derive reports only literal facts; relations needing the real count are left to `validate` before sending.
- **Own JSON walk, twilight's `InteractionDataResolved`:** twilight's modal enum loses types 21-23 (verified in 0.17.1 source); its resolved maps are complete and allowed by AD-12.
- **Fetcher seam:** noctis-ui stays I/O-free; core supplies the HTTP fetcher (Epic 7), tests a fake.
- **Golden example:**
  ```rust
  #[derive(Choices, Clone, Copy, PartialEq, Eq, Debug)]
  enum Severity { #[choice("Low", value = "l")] Low, #[choice("High", value = "h")] High }
  #[derive(Modal, Debug)]
  #[modal(route = Report, title = "Report a user")]
  struct ReportForm {
      #[text_display("Tell us what happened.")] _intro: (),
      #[text_input("Reason", style = paragraph, max_length = 500)] reason: String,
      #[radio_group("Severity")] severity: Severity,
      #[file_upload("Evidence", max_values = 3)] files: Vec<ResolvedAttachment>,
      #[checkbox("I agree")] agree: bool,
  }
  let modal = ReportForm::modal(Report { id: 7 });
  let form = ReportForm::from_json(bytes)?;
  ```

## Verification

**Commands** (repo root; CI is paused, local gates only; never a debug build):
- `cargo fmt --all --check` -- exit 0.
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- exit 0.
- `INSTA_UPDATE=no cargo test --release --workspace` -- exit 0 (unit, decode, snapshot, boundary, trybuild, doctests).
- `cargo hack test --release --workspace --each-feature` -- exit 0.
- `cargo +1.91 check --release --workspace --all-targets` -- exit 0.
- `cargo deny check` -- exit 0.
- `cargo run --release -p xtask -- deps-check`, then `-- rules-catalog`, `-- api-snapshot`, `-- skill-check` -- each exit 0.
- `grep -c 'story = "4.6"' xtask/rules-pending.toml` -- prints 0. `test ! -d target/debug` -- true.

## Auto Run Result

- **Summary:** `#[derive(Modal)]` (noctis-macros) builds a Components V2 modal from a struct (one attribute per field: text_display, text_input, string_select, radio_group, checkbox, checkbox_group, file_upload, entity selects) with compile-time MOD-01..07/CID-01/CV2 checks; noctis-ui decodes raw modal-submit JSON (own walk, types 21-23 that twilight drops) into typed fields, with `SubmitError`, resolved attachments and an I/O-free `FileFetcher` download seam.
- **Files changed:** `crates/noctis-macros/src/{modal,choices}.rs` (derive + Choices counts); `crates/noctis-ui/src/modal/{submit,resolved}.rs` (+tests) (decoder, resolved data, download); `crates/noctis/src/lib.rs` (re-exports); `crates/noctis/tests/{modal_derive,derive_hygiene}.rs`, `tests/boundary/modal.rs`, `tests/fixtures/modal/*.json`, snapshot (decode/snapshot/boundary tests); `crates/noctis/tests/ui/mod/*`, `ui/cv2/CV2-09_derive_string_select_26_choices.*` (compile_fail); `docs/discord-rules.md`, `xtask/rules-pending.toml`, `xtask/public-api.txt` (rules + API snapshot); `examples/project-template/` (AGENTS.md, Usage Skill modals.md/limits/SKILL.md, profile.rs).
- **Review findings:** 34 total (high 0, medium 0, low 21, false 13). Patched 8 low: hard `FileFetcher` contract (error, never truncate, no off-CDN redirects; covers oversized-buffer finding), empty required `Vec` docs reworded to `Invalid`, CV2-09 26-Choices trybuild fixture, File Upload default max_values 10 test, fetcher URL/max_bytes asserted, >100-char implicit choice value test, span-fallback test asserts resolved key. Deferred 0. Rejected 26: 13 false (min/max vs Choices count is runtime per intent-contract; template pre-fill `[planned, Story 9.4]`; nested-wrapper walk safe; handler receipt is Story 7.8; FR-64 fixtures valid; three forms forced by MOD-02; no HTTP fetcher by AD-2/AD-15; unknown data only on `ModalSubmission`; literal texts until Epic 11; decode bounds required) and 13 low rejected as rare/loud-compile-error/forged-payload-only (duplicate values, `Vec` alias, repeated attribute args, channel_types prefixes, cfg-gated variants, unchecked channel/file types, unknown-type key collision).
- **Follow-up review recommended:** false (patched: high 0, medium 0, low 8).
- **Verification:** all local gates green via gates.sh (fmt, clippy -D warnings, test --release, cargo hack each-feature, MSRV 1.91, cargo deny, xtask deps-check/rules-catalog/api-snapshot/skill-check); no debug build. CI paused by maintainer.
- **Residual risks:** min/max vs real option count only caught by runtime `validate`; forged payloads with duplicate values or mismatched channel/file types are accepted; download safety depends on the core HTTP fetcher (Epic 7) honouring the documented contract; pre-fill pending Story 9.4.
