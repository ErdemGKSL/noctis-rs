---
title: 'Story 9.3: Route component defaults with per-render overrides'
type: 'feature'
created: '2026-10-22'
status: 'ready-for-dev'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/_bmad-output/implementation-artifacts/epic-9-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/maintainer-decisions.md'
  - '{project-root}/docs/discord-rules.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/api-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/macros.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/type-design.md'
  - '{project-root}/.claude/skills/rust-best-practices/references/testing.md'
warnings: ['oversized']
deferred: []
---

<intent-contract>

## Intent

**Problem:** Story 6.11 shipped `defaults(..)`, `.build()` and one-field chainable setters. Several pieces are still missing:
- there is no way to override several fields at once;
- there is no `.default_values(..)` on every select;
- there is no localized-or-literal `Text`;
- a path in `defaults(..)` gives a generic "must be a literal" error;
- modals open blank: `X { .. }.build()` ignores the input values, `X::blank(..)` returns a `Modal`, and there is no `.title(..)`, `.value(field, ..)` or overrides struct (FR-59, FR-63; maintainer decisions §7 and §9).

**Approach:**
- Add a public `noctis_ui::Text` (a literal or a `MessageKey`). Every override takes `impl Into<Text>`, and the Responder resolves keys in the response locale.
- Add one overrides struct per message kind, applied with `.overrides(..)`, plus `.default_values(..)` on every select.
- `#[derive(Modal)]` pre-fills inputs from the struct value through a typed `ModalBuilder<X>`, which carries `.title(..)`, `.value(X::FIELD, v)` and a generated `<X>Overrides`.
- `X::blank(..)` returns `Self` when every input has an empty value. Pre-fill limits become validator rules, checked before sending.

## Boundaries & Constraints

**Always:**
- **`Text`** is new, in `noctis-ui` (`src/text.rs`), re-exported as `noctis::ui::Text` and in the prelude.
  - Shape: `#[non_exhaustive] #[derive(Debug, Clone, PartialEq, Eq)] pub enum Text { Literal(Cow<'static, str>), Key(MessageKey) }`.
  - Explicit `From` impls (no blanket impl): `&'static str`, `String`, `Cow<'static, str>`, `MessageKey`, and every `literal.rs` newtype (`Label`, `Placeholder`, …).
  - `as_str(&self) -> &str` returns the literal, or the key as written for `Key`.
  - Rename the private `type Text = Cow<'static, str>` aliases in `limits/lenient.rs`, `modal/{input,select,mod}.rs` to `Str`.
- **Fields stored as `Text`** (replacing `Cow`):
  - `RouteButtonData::label`;
  - `component::select::Common::placeholder`;
  - `Modal::title`;
  - `ModalLabel` label and description;
  - `TextInput::placeholder`;
  - `ModalCommon::placeholder`.

  Their setters (and `ModalLabel::new`) take `impl Into<Text>`. Every other text (TextDisplay, options, LinkButton, values) stays `Cow`.
- **An unresolved `Key`** serializes and lowers as its key string. Lenient truncation turns it into a `Literal`.
- **Key resolution in the Responder.**
  - New hidden `noctis_ui` methods `V2Message::resolve_keys(&mut self, f: &mut dyn FnMut(MessageKey) -> String)`, the same on `LegacyMessage`, and `Modal::resolve_keys`. Each replaces every `Text::Key` in the stored fields above with `Literal(f(key))`.
  - `Shared` stores the App's `Arc<dyn Translate>` (`Builtin` when detached) and the run's user and guild locale codes.
  - `prepare` and `show_modal` call `resolve_keys` before `resolve_ids`/`validate`.
  - Locale order: User then Guild for an ephemeral send and for a modal; Guild then User for a public send. The visibility is the one the send ends with (the source message's `ephemerality` for update and edit paths); Guild first when the Responder does not know it. `en-US` when neither locale parses (reuse `notice::locale`).
  - A key with no translation gives its key string, with one `warn` on target `noctis::i18n`. This is not `notice::text`'s generic-error fallback.
- **`defaults(..)` paths.** In `crates/noctis-macros/src/defaults.rs`, a path value for `label`, `placeholder` or `title` fails with ``"`<key>` is a locale key path: locale keys are [planned, Epic 11]; use a string literal"``, with the rule id prefix CV2-08, CV2-09 or MOD-01. Any other non-literal keeps today's error. Modal field attributes (`LOCALIZED` in `modal.rs`) get the same wording. A string literal stays literal whatever the cargo features.
- **Message overrides structs** in `noctis-ui` (`src/overrides.rs`), re-exported in `noctis::ui` and the prelude. They are `#[derive(Debug, Clone, Default)]`. They are **not** `#[non_exhaustive]`, because `..Default::default()` literals must compile outside the crate. Every field is `Option`, using `Text` for text:
  - `ButtonOverrides { style, label, emoji, disabled }`;
  - `StringSelectOverrides { placeholder, min_values, max_values, disabled, options: Option<Vec<SelectOption>>, default_values: Option<Vec<Cow<'static, str>>> }`;
  - `UserSelectOverrides`, `RoleSelectOverrides` and `ChannelSelectOverrides`: `{ placeholder, min_values, max_values, disabled, default_values: Option<Vec<Id<..Marker>>> }`. `ChannelSelectOverrides` also has `channel_types: Option<Vec<ChannelType>>`;
  - `MentionableSelectOverrides`: the same, with `default_values: Option<Vec<MentionableId>>`.
- **`.overrides(o)` and the setters.**
  - `.overrides(o)` is `#[must_use]` on `RouteButton<R>`, `StringSelect`, `UserSelect`, `RoleSelect`, `MentionableSelect` and `ChannelSelect`. It applies each `Some` field exactly as its setter would, in declaration order. Setters and `.overrides` compose, and the later call wins.
  - `StringSelectBuilder` gets no `.overrides`. It is available once `.options(..)` gives a `StringSelect`.
  - Overrides change one render only; `DEFAULTS` is a const.
- **`.default_values(..)` on every select.**
  - New `#[non_exhaustive] pub enum MentionableId { User(Id<UserMarker>), Role(Id<RoleMarker>) }`.
  - On `StringSelect` and `StringSelectBuilder`, `default_values(impl IntoIterator<Item = impl Into<Cow<'static, str>>>)` stores the value list. On the wire and in lowering, the options whose value is listed have `default: true` and all others have none, whatever the setter order; without a list, the options' own flags apply.
  - On the entity selects, `default_values(..)` takes ids (`MentionableId` for Mentionable) and replaces the list. It is equal to `default_users`/`default_roles`/`default_channels`, which stay.
  - Duplicates are removed, keeping the first occurrence; an empty list sends no `default_values` key.
- **New validator rules** (`noctis-rules`, plus `noctis-ui` lowering; every mode errors, nothing is truncated):
  - CV2-09 at `<path>.default_values[i]` for a String Select list value that no option has. The Bound is `Describe("a value of one of the options")` and the Actual is the value.
  - MOD-05 at `<path>.value` when a Text Input value is longer than its own `max_length` (`AtMost(max_length)`, `discord_len` counting). This adds to the existing ≤ 4000 check. A value shorter than `min_length` is not checked.
  - MOD-07 for a Checkbox Group with at least one default: the default count must lie in the effective `[min_values, max_values]`, at `<path>.default_values`.
  - MOD-07 for a Radio Group with more than one default (`AtMost(1)`).
  - Modal-select default counts already go through CV2-09 `check_select`: keep it and test it.
- **`#[derive(Modal)]`** (`crates/noctis-macros/src/modal.rs` plus `noctis-ui` helpers) generates:
  - `X { .. }.build() -> ModalBuilder<X>`, pre-filled from every input field value;
  - `X::blank(<payload fields>)`:
    - it returns `Self` when every input field type is syntactically `Option<_>`, `Vec<_>`, `String`, `bool` or `()`, with values `None`, `vec![]`, `String::new()`, `false` and `()`;
    - otherwise it returns `ModalBuilder<X>` with empty inputs. This covers a required single `Choices` enum or `Resolved*`/`Mentionable`; type aliases count as non-empty-able;
  - `impl From<X> for Modal` (≡ `x.build().into()`);
  - one `pub const <FIELD_SCREAMING>: InputField<X, T>` per input field, where `T` is the field type, except for `#[text_display]` and `#[file_upload]`;
  - `pub struct <X>Overrides { pub title: Option<Text>, pub <input>: InputOverrides, … }` with the struct's visibility, `Debug, Clone, Default`, and one field per Label input (every kind except `#[text_display]`).

  These are compile errors in the derive (plain `syn::Error`, no rule id):
  - an input field whose unraw name is `title`;
  - an input whose SCREAMING name equals an associated const the derive's traits define (`ID`, `SHAPE`, `SCHEMA`, `DEFAULTS`, `RUNTIME`, `ROUTE`, `RESPONSE`; confirm the list by grep).
- **`noctis_ui::ModalBuilder<X>`** (public, `#[must_use]`, `Clone`, `Debug`).
  - It holds the Payload, a hidden generated prefill struct (`Option<T>` per input field) and `X::Overrides`, through a hidden trait `ModalRender` (associated `Payload`, `Prefill`, `Overrides`; `fn render(..) -> Modal`).
  - Methods:
    - `title(impl Into<Text>)`;
    - `value<T>(self, field: InputField<X, T>, value: T)`;
    - `overrides(X::Overrides)`, which merges per field: each `Some` replaces, `InputOverrides` merges field by field, and the later call wins.
  - `impl From<ModalBuilder<X>> for Modal`.
  - `InputOverrides { label, description, placeholder: Option<Text> }` (`Default`, not `non_exhaustive`). `placeholder` is ignored for File Upload, Radio Group, Checkbox Group and Checkbox (documented).
- **Pre-fill mapping.** A hidden method per field trait in `modal/form.rs` gives the component the value of the field:
  - Text Input: a non-empty `String`/`Some(s)` becomes `value`, and `""`/`None` sends no `value`;
  - String Select, Radio Group and Checkbox Group: each chosen `Choices::value()` gets option `default: true`;
  - User, Role and Channel selects: the resolved ids become `default_values`;
  - Mentionable: user and role ids;
  - Checkbox: `true` becomes `default: true`;
  - File Upload and Text Display: nothing.

  Pre-fill never changes the `custom_id`, the decode or the input specs.
- **Core.**
  - `show_modal` (typestate macro, `responder.rs` around L961, and `Shared::show_modal`) takes `impl Into<Modal>`.
  - A pre-fill or override that breaks a limit returns `ResponderError::Invalid(UiError::Limits{..})` and sends no request.
- **Facade, docs, template and snapshot**, all in the same change:
  - `noctis::ui::{Text, ButtonOverrides, StringSelectOverrides, UserSelectOverrides, RoleSelectOverrides, MentionableSelectOverrides, ChannelSelectOverrides, InputOverrides, ModalBuilder, InputField, MentionableId}`; the prelude adds `Text`, the six message overrides and `InputOverrides`;
  - `docs/discord-rules.md` rows CV2-08, CV2-09, MOD-01, MOD-04, MOD-05 and MOD-07 (How column: overrides, `Text`, pre-fill checks, test names);
  - the skill: `modals.md`, `routes-payloads-refs.md` §1 and §9, `components-v2.md` (L132, L370, L398, L470, L763 `[planned: Story 9.3]` notes), `views-and-recipes.md` L90, L876–884 and L1426, `commands.md`/`responder.md` where `show_modal(Modal)` appears, and the `SKILL.md` API hash;
  - `xtask/src/skill_check/components.rs` `BUILDERS` gains `"ModalBuilder"`, with a `modals.md` entry having **Where:**, **Limits:** and **Builder:**;
  - the template: `/bio` uses `r.show_modal(BioForm::blank(ctx.user_id()))`, and `EditBio` uses `BioForm { bio: stored.bio, pronouns: stored.pronouns, ..BioForm::blank(self.user) }.build()`; drop the `[planned, Story 9.4]` notes in `src/` and `AGENTS.md`;
  - re-bless `xtask/public-api.txt`;
  - the spine gets an "Amended (Story 9.3)" bullet under AD-19 (overrides, `Text`, `ModalBuilder`, `blank` rule).

**Never:**
- `#[non_exhaustive]` on an overrides struct.
- A blanket `impl<T: Into<Cow>> From<T> for Text`.
- Truncating or changing a pre-filled value, a default value or a count in Lenient mode.
- Localizing TextDisplay, option or LinkButton text (Epic 11).
- Accepting a `keys::*` path in an attribute (Story 11.1).
- New crate dependencies, webhook work (9.5), editing `_bmad/`, or weakening `.cargo/config.toml`.

## I/O & Edge-Case Matrix

| # | Scenario | Input / State | Expected | Error |
|---|---|---|---|---|
| T1 | Text from | `"a"`, `String::from("a")`, `Cow::Borrowed("a")`, `label!("a")`, `MessageKey::new("x.y")` | `Literal("a")` ×4; `Key(x.y)` | — |
| T2 | Unresolved key | `ShopBuy{..}.build().label(MessageKey::new("shop.buy"))` serialized directly | `"label":"shop.buy"` | — |
| T3 | Locale pick | strings fake: `shop.buy` tr=`Al`, de=`Kaufen`; user `tr`, guild `de` | ephemeral reply `Al`; public `Kaufen`; modal title key → `Al`; update of an ephemeral source `Al`; user `xx` and guild none → en-US | — |
| T4 | Missing key | key with no translation | key string sent, one `warn` on `noctis::i18n` | — |
| T5 | Resolved too long | translation of 81 chars as a label | Strict CV2-08 `Invalid`, no request; Lenient: 80 chars ending `…` | `ResponderError::Invalid` |
| T6 | Unicode bounds | label override of 80 `😀`; 81 | ok; CV2-08 (`discord_len`) | `UiError::Limits` |
| D1 | Path in defaults | `label = keys::SAVE`; `placeholder = keys::P`; modal `title = keys::T`; `#[text_input(keys::L)]` | compile errors `[CV2-08]`/`[CV2-09]`/`[MOD-01]`/`[MOD-04]` containing `[planned, Epic 11]` (trybuild) | compile error |
| D2 | Existing checks | missing block or key, unknown key, 81-char label, `""` label | unchanged errors (existing fixtures pass unmodified) | compile error |
| B1 | No overrides | `Buy{item:1}.build()`; `.overrides(ButtonOverrides::default())` | both JSON equal the defaults | — |
| B2 | Multi override | `.overrides(ButtonOverrides{label:Some("Again".into()),disabled:Some(true),..Default::default()})` | label and disabled changed; style, emoji and `custom_id` from defaults | — |
| B3 | Later wins | `.label("a").overrides(label b)` → `b`; `.overrides(label b).label("a")` → `a`; `.style(Danger).overrides(style Some(Success))` → 3 | as stated | — |
| B4 | One render only | build with overrides, then a second `Buy{item:1}.build()` | second equals the defaults | — |
| B5 | Emoji/style | `emoji: Some(Emoji::unicode("🔥"))`, `style: Some(Danger)` | `emoji.name` 🔥, `style` 4 | — |
| S1 | Placeholder bounds | 150 / 151 chars, `Key` | ok / Strict CV2-09, Lenient truncated to 150; key resolved | `UiError::Limits` |
| S2 | Counts | `max_values` 25 / 26; `min_values` 0 / 26; min 3 > max 2 | ok / CV2-09 in both modes | `UiError::Limits` |
| S3 | Options override | `StringSelectOverrides{options:Some(vec![a]),..}`; `Some(vec![])`; 26 options | replaces the declared options; CV2-09 (both modes); CV2-09 | `UiError::Limits` |
| S4 | String defaults | options a,b,c with `a.default(true)`; `.default_values(["b"])`; also `.default_values(["b"])` before `.options(..)` on the builder | only `b` has `default:true` in both orders | — |
| S5 | Unknown default | `.default_values(["z"])` | CV2-09 at `…default_values[0]`, Actual `z`, Strict and Lenient | `UiError::Limits` |
| S6 | Default count | 2 values with max 1; 2 values with min 2, max 3; duplicates `["b","b"]` | CV2-09; ok; one default | `UiError::Limits` |
| S7 | Entity defaults | User `.default_values([1,2])` max 2; `[1,2,3]` max 2; `[]`; `[1,1]` | `[{"id":"1","type":"user"},{"id":"2",..}]`; CV2-09; no key; one entry | `UiError::Limits` |
| S8 | Mentionable | `[MentionableId::User(1), MentionableId::Role(2)]` | `type` `user`, `role` in order | — |
| S9 | Channel | `ChannelSelectOverrides{channel_types:Some(vec![GuildText]),default_values:Some(vec![9]),..}` | `channel_types:[0]`, `default_values:[{"id":"9","type":"channel"}]` | — |
| S10 | Builder has no overrides | `TicketKind.build().overrides(..)` | E0599 (`compile_fail` doctest on `StringSelectBuilder`) | compile error |
| M1 | Text pre-fill | `BioForm{user, bio:Some("hi".into())}.build()` | text input `"value":"hi"` | — |
| M2 | Empty text | `None`; `Some("")`; required `String::new()` | no `value` key | — |
| M3 | Radio / checkbox | `pronouns: Some(He)`; `None`; `agree: true`; `false` | option `he` `default:true`, others without; none; checkbox `default:true`; absent | — |
| M4 | Groups and string select | Checkbox Group `vec![A,C]`, `vec![]`; modal String Select `Colour::Red` | A and C default; none; `red` default | — |
| M5 | Entity pre-fill | `Vec<ResolvedUser>` [u1,u2], `ResolvedRole` r, `Vec<Mentionable>` [user u, role r], `Option<ResolvedChannel>` Some(c) | `default_values` with ids and types `user`/`role`/`channel` | — |
| M6 | No pre-fill kinds | `Vec<ResolvedAttachment>` with 2; `()` text display | File Upload without defaults; Text Display unchanged | — |
| M7 | Blank → Self | BioForm (`Option` inputs); form with `String` + `bool` + `Vec` | `blank(user): BioForm` with empty values; `BioForm{bio:Some(v),..BioForm::blank(user)}` compiles; `Modal::from(BioForm::blank(u))` has no values | — |
| M8 | Blank → builder | EntityForm (required `ResolvedRole`), ReportForm (required `Severity`) | `blank(..)` is `ModalBuilder<_>`; JSON equals today's blank snapshot | — |
| M9 | Title override | `.title("x")`; 45 / 46 chars; `MessageKey` | `"title":"x"`; ok / MOD-01 Strict error, Lenient truncated; resolved | `UiError::Limits` |
| M10 | value() | `x.build().value(BioForm::BIO, Some("new".into()))`; on a blank builder; twice (later wins) | `value:"new"`; set; last value | — |
| M11 | Input overrides | `BioFormOverrides{bio:InputOverrides{label:Some("About".into()),..Default::default()},..Default::default()}` | Label text `About`; the `value` from the struct stays; other inputs unchanged | — |
| M12 | Override merge | `.overrides(bio.label a).overrides(bio.description d)`; `.title("a").overrides(title b)` | label a and description d both kept; title b | — |
| M13 | Override bounds | Label 45/46 and description 100/101 (MOD-04); text placeholder 100/101 (MOD-05); select placeholder 150/151 (CV2-09); placeholder on a radio | ok / Strict error, Lenient truncated; radio unchanged | `UiError::Limits` |
| M14 | Value 4000 | no `max_length`: 4000 / 4001 chars | ok / MOD-05 at `…value`, both modes | `UiError::Limits` |
| M15 | Over max_length | `max_length = 300`: 300 / 301 chars; 300 `ü` | ok / MOD-05 `AtMost(300)` both modes; ok | `UiError::Limits` |
| M16 | Too many defaults | Checkbox Group `max_values = 2` with 3 chosen; User Select `max_values = 3` with 4; radio via `ModalOption::default` ×2 on a runtime `RadioGroup` | MOD-07; CV2-09; MOD-07 `AtMost(1)` | `UiError::Limits` |
| M17 | Before sending | `r.show_modal(<M14/M15/M16 value>)` | `ResponderError::Invalid(UiError::Limits)`; fake log has no callback | `ResponderError::Invalid` |
| M18 | Reserved names | input field `title`; input field `id` (SCREAMING `ID`) | compile error naming the field and why (derive unit test in `modal.rs`) | compile error |
| M19 | Decode unchanged | submit JSON of a pre-filled modal | decodes as before (`kinds.rs` Matrix 32 still passes) | — |
| M20 | Into<Modal> | `show_modal(x.build().title(..))`, `show_modal(X::blank(p))` (both variants), `show_modal(modal)` | all compile and send type 9 | — |

</intent-contract>

## Code Map

- `crates/noctis-ui/src/route.rs`: `ComponentRoute`, `*Defaults`, the `build_*` fns (L160–250), `build_modal` (L252). The defaults stay `&'static str` and are wrapped as `Text::Literal`.
- `crates/noctis-ui/src/component/button.rs` L100–160: `RouteButtonData`, the `RouteButton` setters, `Wire`.
- `crates/noctis-ui/src/component/select.rs`:
  - L86–113 `Common` / `apply`;
  - L115 `DefaultValue` (private; `MentionableId` maps to it);
  - L150 `select_map`;
  - L200–245 `common_setters!`;
  - L250–360 `StringSelect` / `StringSelectBuilder`;
  - L368–500 the entity selects;
  - L558–600 `SelectNode::common`/`parts_mut`/`default_count`.
- `crates/noctis-ui/src/modal/{mod.rs (Modal L95, ModalLabel L216), input.rs (TextInput L51, ModalOption L223, RadioGroup, CheckboxGroup, Checkbox), select.rs (ModalCommon L23, setters L66)}`.
- `crates/noctis-ui/src/modal/form.rs`:
  - field traits L455–730 (add the pre-fill accessor; `Shape<I>` L214–260);
  - builder fns L738–936 (add the pre-fill parameter).
- `crates/noctis-ui/src/limits/{lower.rs, lenient.rs, walk.rs, mod.rs}`: lowering text and default counts (String default list), Lenient truncation of `Text`.
- `crates/noctis-ui/src/literal.rs` L26–65: the newtypes (`From<$name> for Cow`); add `From<$name> for Text`.
- `crates/noctis-ui/src/lib.rs` L298: the `__macro` re-exports; add `ModalRender`, `InputField::new`, the prefill helpers and the public types.
- `crates/noctis-rules/src/modal.rs`:
  - `check_text_input` L573 (value vs `max_length`);
  - the radio arm L535;
  - `check_checkbox_group` L731 (default count);
  - `ModalOption::default` L312.
- `crates/noctis-rules/src/component.rs` L865–935 `check_select`: the default count exists; add the unknown-default input (the `Select` descriptor gets the String Select default list, or the lowering reports it).
- `crates/noctis-macros/src/defaults.rs`:
  - `text()` L169 and `literal_error` L162: the path error;
  - tests L613+.
- `crates/noctis-macros/src/modal.rs`:
  - `LOCALIZED` L31;
  - `component()` L849 (pass the pre-fill);
  - `expand()` L985–1105: `build`/`blank`/consts/Overrides/`ModalRender`/`From`;
  - tests L1110+.
- `crates/noctis-core/src/responder.rs`:
  - typestate `show_modal` L961;
  - `Shared` L1278 and `new` L1330;
  - `SourceMessage.ephemerality` L1165;
  - `default_ephemerality` L1458;
  - `prepare` L1648;
  - `Shared::show_modal` L1965.
- `crates/noctis-core/src/pipeline.rs` L904 and L1544, and `command.rs` L728: `Shared` constructors, which pass strings and locales (`CtxInner.user_locale`/`guild_locale` L108).
- `crates/noctis-core/src/notice.rs` L12 `locale`; `responder/ergonomics.rs` L158–186 `resolve`/`text_in`, the model for the locale order.
- Tests to extend:
  - `crates/noctis-ui/tests/defaults.rs` (hand-written routes, Matrix 29–49);
  - `crates/noctis/tests/kinds.rs` (L250 overrides, L311 modal: replace the "pre-fill arrives with Story 9.4" assertion);
  - `crates/noctis/tests/modal_derive.rs` (TextForm/EntityForm/UploadForm `r#title` field L125: rename it, M18);
  - `crates/noctis/tests/boundary/modal.rs`, `modal_flow.rs` L286, `derive_hygiene.rs` L232;
  - `crates/noctis-testkit/tests/{components.rs L246/L1132, testapp.rs L590}`;
  - `crates/noctis-core/src/responder/tests/components.rs` (fixtures with a recording upstream).
- trybuild: `crates/noctis/tests/ui/cv2/CV2-08_defaults_label_81.rs` and `CV2-09_defaults_placeholder_151.rs` (the models), `tests/ui/mod/MOD-01_title_too_long.rs`.
- Template: `examples/project-template/src/modules/profile/{handlers.rs L50–60 and L134–147, mod.rs L90–107}`; skill files under `examples/project-template/.claude/skills/noctis/references/` (lines in Boundaries).

## Tasks & Acceptance

**Execution:**
- `crates/noctis-ui/src/text.rs` (new), `lib.rs`, `literal.rs`: `Text` with its `From` impls and `as_str`; rename the private aliases. Unit tests T1 and T2.
- `crates/noctis-ui/src/component/{button.rs,select.rs}`, `src/overrides.rs` (new):
  - switch the stored fields to `Text`;
  - add the overrides structs, `.overrides`, `.default_values`, `MentionableId` and the `resolve_keys` walks;
  - doc examples and a `compile_fail` doctest for S10.
- `crates/noctis-rules/src/{modal.rs,component.rs}`: the MOD-05 value-vs-`max_length` rule, the MOD-07 default rules and the CV2-09 unknown-default rule. Unit tests named `mod_05_*`, `mod_07_*` and `cv2_09_*`.
- `crates/noctis-ui/src/limits/*`: lowering and Lenient handling for `Text` and the String default list. Tests S1–S7 and M13–M16 in `crates/noctis-ui/tests/validate.rs` and `tests/defaults.rs`.
- `crates/noctis-ui/src/modal/{mod.rs,input.rs,select.rs,form.rs}`, `route.rs`: the `Text` fields, `Modal::resolve_keys`, the pre-fill accessors and parameters, and `ModalBuilder`/`InputField`/`InputOverrides`/`ModalRender`.
- `crates/noctis-macros/src/{defaults.rs,modal.rs}`:
  - the path errors (D1);
  - the generated `build`/`blank` (Self or builder), consts, `<X>Overrides`, `From<X> for Modal` and `ModalRender`;
  - the reserved-name errors.

  Add macro unit tests for M18 and the blankable classification.
- `crates/noctis/tests/ui/{cv2,mod}/`: new fixtures `CV2-08_defaults_label_path.rs`, `CV2-09_defaults_placeholder_path.rs`, `MOD-01_defaults_title_path.rs` and `MOD-04_text_input_label_path.rs`, with blessed `.stderr`. Update any `.stderr` the wording change touches.
- `crates/noctis/tests/{kinds.rs,modal_derive.rs,boundary/modal.rs,...}` and the testkit tests: B1–B5 and M1–M12, M19 and M20 through the facade derives. Insta snapshots for one pre-filled modal per form kind. Update the `blank` call sites.
- `crates/noctis-core/src/{responder.rs,pipeline.rs,command.rs}`:
  - strings and locales on `Shared`;
  - key resolution in `prepare`/`show_modal`;
  - `show_modal(impl Into<Modal>)`.

  Responder tests T3–T5 and M17 (a scripted `Translate`; the recording upstream shows no request on error).
- `crates/noctis/src/lib.rs`: the re-exports, the prelude, and module docs (L2234–2410 `blank` examples).
- `xtask/src/skill_check/components.rs`: add `"ModalBuilder"`.
- `docs/discord-rules.md`, the ARCHITECTURE-SPINE (AD-19 amendment), the template `src/` and `AGENTS.md`, the skill files listed in Boundaries: document overrides (with an `..Default::default()` example), `.default_values`, `Text`, pre-fill with `..X::blank(..)`, `.value`/`.title`/`<X>Overrides`, the `blank` rule and the pre-fill limits; remove every `[planned: Story 9.3]` and `[planned, Story 9.4]` note.
- `xtask/public-api.txt`: re-bless it and update the `API hash`.

**Acceptance Criteria:**
- Given `#[button("shop.buy", defaults(style = Secondary, label = "Buy"))] struct ShopBuy { item: u32 }`, when `ShopBuy { item }.build().overrides(ButtonOverrides { label: Some("Buy again".into()), disabled: Some(true), ..Default::default() })` is sent with `ctx.respond(view)`, then the type-4 body's button has that label, `disabled: true`, style 2 and the Route `custom_id`.
- Given the template's "Edit bio" button with a stored bio, when it is clicked through the TestApp, then the type-9 callback's bio Text Input carries the stored `value` and the pronouns Radio Group marks the stored option `default: true`.
- Given `/bio` in the template, when it runs, then the modal opens with no `value` and no default.
- Given a modal whose pre-filled bio exceeds `max_length = 300`, when `show_modal` is awaited, then `ResponderError::Invalid` naming MOD-05 is returned and no callback is recorded.
- Given a `MessageKey` label override and a `Translate` holding it, when the reply is ephemeral, then the label is the User Locale text.
- Given `defaults(label = keys::SAVE)`, when it compiles, then the error names CV2-08 and `[planned, Epic 11]`.
- Given the finished story, when the local gates run, then `rules-catalog`, `api-snapshot` and `skill-check` all pass, with a `ModalBuilder` entry and a matching API hash.

## Spec Change Log

## Review Triage Log

## Design Notes

- **`blank` return type.** The story and decision §7 want `blank` for "forms whose required inputs have no empty value", while §9 (newer, binding) wants `..X::blank(user)`, which needs `Self`. Both hold:
  - the derive returns `Self` when every input has an empty value (decided syntactically, since a trait bound would fail at the definition for required `ResolvedUser`/`Choices` fields);
  - otherwise it returns the empty `ModalBuilder`.

  `From<X> for Modal` makes `show_modal(X::blank(p))` read the same either way. An empty `String` counts as "no pre-fill".
- **`.value(X::FIELD, v)`.** Typed associated consts keep `.value` checked by rustc without string keys. The field type is the value type, so `.value` and struct-update pre-fill agree.
- **Text syntax over features (§9).** This replaces AD-16's "literal without `i18n`, key with it" for `defaults`. Runtime keys resolve in the Responder, the only place that knows the locale and `Translate`. A missing key shows the key instead of the generic error text, because a button labelled "Something went wrong" is worse. Bot catalogs arrive in Epic 11; today only `noctis.*` keys resolve.
- **Overrides structs are exhaustive** on purpose (api-design §3): adding a field later is a breaking change, which is accepted before 1.0.
- Example:
  ```rust
  let form = BioForm { bio: stored.bio, ..BioForm::blank(user) }.build()
      .title("Edit your bio")
      .overrides(BioFormOverrides { bio: InputOverrides { placeholder: Some("Max 300".into()), ..Default::default() }, ..Default::default() });
  r.show_modal(form).await?;
  ```

## Verification

**Commands:**
- `cargo fmt --all` -- no diff
- `cargo clippy --release --workspace --all-targets --all-features -- -D warnings` -- clean
- `cargo test --release --workspace` -- all pass (ui, rules, macros, core responder, facade derives, trybuild, doctests)
- `cargo hack test --release --workspace --each-feature` -- all pass
- `cargo run --release -p xtask -- deps-check` -- ok (no new edges)
- `cargo run --release -p xtask -- rules-catalog` -- ok (new fixtures and rule test names)
- `cargo run --release -p xtask -- api-snapshot --bless`, then `cargo run --release -p xtask -- api-snapshot` -- matches (install `nightly-2026-10-15` and `cargo-public-api` 0.52.0 first if missing)
- `cargo run --release -p xtask -- skill-check` -- ok
- `cargo deny check` -- ok
- `cargo +1.91 check --release --workspace` -- ok
- `test ! -d target/debug` -- no debug profile
