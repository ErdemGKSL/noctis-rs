# Limits and rule ids cheatsheet

Every numeric limit Noctis enforces, with its rule id (the id appears in compile
errors, `BuildError`s and runtime errors). Source of truth: the Discord Rules Catalog
(`docs/discord-rules.md` in the noctis-rs repository). **Where:** C = compile time,
B = App build, R = runtime. [U] = counting unit/value unconfirmed by Discord; Noctis
counts the larger of Unicode scalars and UTF-16 units and keeps the value in `Limits`.

## Limits value

```rust
use noctis::rules::{LimitMode, Limits};

let mut limits = Limits::default();     // Discord's defaults, LimitMode::Strict
limits.mode = LimitMode::Lenient;       // truncate text instead of failing
limits.components = 30;                 // lower a limit (stricter than Discord)
builder.limits(limits);                 // [planned] App builder hook, in `configure`
```

`Limits` is `#[non_exhaustive]`: always start from `Limits::default()` and assign
fields; a struct literal does not compile. Every field is a `usize` maximum.

- `LimitMode::Strict` (default) fails before sending with a `Violation`; nothing
  reaches Discord.
- `LimitMode::Lenient` truncates over-long display-text length fields with `…` (via
  `truncate`) and warns. Counts, byte sizes, patterns, URLs, values and ids always
  fail, and so do the structural rules CV2-03, CV2-05, CV2-06, CID-01 and CID-02.
  `limits.truncates(rule)` answers only whether the mode permits truncation for that
  rule: `true` only in Lenient mode and only for a non-structural rule.
- Autocomplete is always truncated to 25 (INT-07), and labels to 100, in both modes;
  `truncates` does not govern it (it returns `false` for INT-07 in Strict mode).
- Macros check literals against the defaults. Lowering a field makes the runtime
  re-check every tree, static ones included; raising one never relaxes a
  compile-time check.
- A built message is checked with `noctis::ui::validate(&mut msg, &limits)` (it
  truncates in Lenient mode) and measured with `noctis::ui::cost(&msg, &limits)`;
  see `components-v2.md` §16.

| Field | Rule | Default | Maximum of |
| --- | --- | --- | --- |
| `command_name_len` | CMD-02 | 32 | characters per command name word |
| `command_description_len` | CMD-04 | 100 | characters in a command description |
| `command_size` | CMD-05 | 8000 | characters per command, options and longest localizations included |
| `command_path_words` | CMD-10 | 3 | words in a command path (command, group, subcommand) |
| `command_options` | CMD-13 | 25 | options, subcommands or groups per level |
| `pattern_expansions` | CMD-15 | 25 | expansions of one command name pattern |
| `context_menu_name_len` | CTX-01 | 32 | characters in a context-menu command name |
| `chat_input_commands` | REG-01 | 100 | chat-input commands per scope (also REG-02) |
| `user_commands` | REG-01 | 15 | user commands per scope (also REG-02) |
| `message_commands` | REG-01 | 15 | message commands per scope (also REG-02) |
| `entry_point_commands` | REG-01 | 1 | entry-point commands per scope (also REG-02) |
| `option_length` | OPT-06 | 6000 | `min_length` / `max_length` of a string option |
| `option_choices` | OPT-08 | 25 | choices per option |
| `choice_name_len` | OPT-09 | 100 | characters in a choice name |
| `choice_value_len` | OPT-09 | 100 | characters in a string choice value |
| `option_file_types` | OPT-13 | 10 | `file_types` entries of an attachment option |
| `autocomplete_choices` | INT-07 | 25 | autocomplete choices (always truncated, in every mode) |
| `user_install_followups` | INT-11 | 5 | follow-ups for a user-installed app outside the guild |
| `custom_id_len` | CID-01 | 100 | characters in a `custom_id` (also MOD-08) |
| `components` | CV2-03 | 40 | components per message, nested ones included |
| `text_total_len` | CV2-04 | 4000 | characters across all Text Displays of a message |
| `row_buttons` | CV2-06 | 5 | buttons per Action Row |
| `section_texts` | CV2-07 | 3 | Text Displays per Section |
| `button_label_len` | CV2-08 | 80 | characters in a button label |
| `link_url_len` | CV2-08 | 512 | characters in a link button URL |
| `select_placeholder_len` | CV2-09 | 150 | characters in a select placeholder |
| `select_options` | CV2-09 | 25 | options per String Select |
| `select_values` | CV2-09 | 25 | `max_values` of a select |
| `select_option_label_len` | CV2-09 | 100 | characters in a select option label |
| `select_option_value_len` | CV2-09 | 100 | characters in a select option value |
| `select_option_description_len` | CV2-09 | 100 | characters in a select option description |
| `gallery_items` | CV2-10 | 10 | items per Media Gallery |
| `gallery_description_len` | CV2-10 | 1024 | characters in a Media Gallery item description |
| `thumbnail_description_len` | CV2-11 | 1024 | characters in a Thumbnail description |
| `legacy_action_rows` | CV2-15 | 5 | Action Rows in a legacy (non-V2) message |
| `media_url_len` | CV2-17 | 2048 | characters in a Media Gallery item or Thumbnail URL |
| `modal_title_len` | MOD-01 | 45 | characters in a modal title |
| `modal_components` | MOD-02 | 5 | top-level components of a modal |
| `label_len` | MOD-04 | 45 | characters in a Label |
| `label_description_len` | MOD-04 | 100 | characters in a Label description |
| `text_input_len` | MOD-05 | 4000 | characters in a Text Input value (`min_length` / `max_length`) |
| `text_input_placeholder_len` | MOD-05 | 100 | characters in a Text Input placeholder |
| `file_upload_values` | MOD-06 | 10 | files per File Upload (`max_values`) |
| `file_upload_file_types` | MOD-06 | 10 | `file_types` entries of a File Upload |
| `radio_options` | MOD-07 | 10 | options per Radio Group |
| `checkbox_group_options` | MOD-07 | 10 | options per Checkbox Group |
| `checkbox_group_values` | MOD-07 | 10 | `max_values` of a Checkbox Group |
| `modal_option_text_len` | MOD-07 | 100 | characters in a Radio or Checkbox Group option label, value or description |
| `content_len` | MSG-01 | 2000 | characters in legacy message `content` |
| `mention_users` | MSG-04 | 100 | user ids in allowed mentions |
| `mention_roles` | MSG-04 | 100 | role ids in allowed mentions |
| `nonce_len` | MSG-06 | 25 | characters in a message nonce |
| `sticker_ids` | MSG-06 | 3 | stickers per message |
| `attachment_bytes` | ATT-01 | 20,971,520 | bytes per file (20 MiB; an interaction's `attachment_size_limit` wins) |
| `request_bytes` | ATT-02 | 26,214,400 | bytes per request (25 MiB) |
| `attachments` | ATT-03 | 10 | attachments per message |
| `attachment_description_len` | ATT-06 | 1024 | characters in attachment alt text |
| `webhook_name_len` | WH-04 | 80 | characters in a webhook name |

REG-01 counts also apply per guild (REG-02); `custom_id_len` also covers modal ids
(MOD-08). Minima (0, 1, 2), protocol constants, timings, rate limits and codec widths
are not `Limits` fields.

## Rules types

All in `noctis::rules`; the command pattern, tree, option, context-menu and metadata
functions live in `noctis::rules::command` (see `commands.md` §2 "Rule functions", §3
"Rule functions (options)", §5 and §6 "Metadata rules"):

| Item | What it is |
| --- | --- |
| `Limits`, `LimitMode` | the limits above and Strict/Lenient handling |
| `command::{expand, validate_name, validate_tree, validate_merge}` | Command Patterns and trees (`commands.md` §2) |
| `command::{validate_options, measure_options, OptionDecl, OptionType, Choice, ChoiceValue, NumericValue}` | options (`commands.md` §3) |
| `command::validate_context_menu_name` | CTX-01 (`commands.md` §5) |
| `command::{validate_meta, CommandMeta, InteractionContext, IntegrationType}` | base-command metadata (`commands.md` §6) |
| `command::validate_autocomplete` | an autocomplete response: INT-07 count, OPT-09/03/04 per kept choice, OPT-11 (`commands.md` §4) |
| `component::{validate_components, validate_legacy_components, cost, Component, Container, Section, Accessory, Media, Separator, Spacing, Button, ButtonStyle, Select, SelectKind, SelectOption, Cost}` | Components V2 trees and legacy rows, CV2-03..13, CV2-15..17 (`components-v2.md` §15) |
| `component::{validate_button_label, validate_link_url, validate_select_placeholder, validate_gallery_description, validate_thumbnail_description, validate_media_url, validate_accent_color, validate_spacing}` | single CV2 values (path `""`) (`components-v2.md` §15) |
| `modal::{validate_modal, Modal, ModalComponent, Label, LabelChild, TextInput, TextInputStyle, ModalSelect, FileUpload, RadioGroup, CheckboxGroup, Checkbox, ModalOption}` | modals, MOD-01..07 and CV2-09 for modal selects (`modals.md` §7) |
| `modal::{validate_modal_title, validate_label_text, validate_label_description, validate_text_input, validate_text_input_value, validate_input_custom_id}` | single modal values; `validate_input_custom_id` is CID-01 for a modal input key (`modals.md` §7) |
| `message::{validate_content, validate_mentions, validate_nonce, validate_sticker_ids, validate_v2_not_empty, validate_legacy_not_empty, validate_webhook_name, AllowedMentions, MentionIds}` | MSG-01, MSG-03..06, MSG-09, WH-04 (`components-v2.md` §15) |
| `attachment::{validate_attachments, validate_file_size, validate_attachment_description, validate_attachment_ref, AttachmentDecl}` | ATT-01..03, ATT-05, ATT-06 (`components-v2.md` §15) |
| `RuleId` | a catalog id: consts `RuleId::CV2_03`, `RuleId::META_05`, … (the id with `-` → `_`); `RuleId::ALL` (126, catalog order); `as_str()` / `Display` → `"CV2-03"`; `area()` → `"CV2"`; `number()` → `3`; `RuleId::lookup("CMD-01")` (exact, case-sensitive) → `Option<RuleId>`; `is_structural()` |
| `Violation` | `{ rule, path, limit, actual, notes: Vec<String>, help: Option<String> }`, `#[non_exhaustive]`, built with `Violation::new(rule, path, limit, actual)` plus `.with_note(…)` (appends) and `.with_help(…)` (replaces); implements `Error`; prints `[CV2-03] components: limit at most 40, actual 41` (no `path: ` part when the path is empty), then a `\n  = note: …` line per note and a `\n  = help: …` line |
| `Bound` | the broken limit: `AtMost(n)` "at most n", `AtLeast(n)` "at least n", `Between { min, max }` "between min and max", `Exactly(n)` "exactly n", `Describe(&str)` printed as is |
| `Actual` | the offending value: `Count(n)` printed as `n`, `Text(String)` printed escaped in backticks |
| `discord_len(&str) -> usize` | the only length function for user text: the larger of Unicode scalars and UTF-16 units (`discord_len("😀") == 2`, `"👍🏽"` = 4) |
| `truncate(&str, max) -> Cow<str>` | fits text into `max` `discord_len` units, ending with `…` (one unit); unchanged and borrowed when it fits; cuts between scalars (not grapheme-aware) |
| `Locale` | Discord's 32 codes, one variant per code (table below); `Locale::ALL`; `as_str()` / `Display` → the code; `FromStr` is exact and case-sensitive |
| `ParseLocaleError` | `input()`, `suggestions()` (codes with the same language part); prints `` [LOC-01] `en` is not a Discord locale code; matching codes: `en-GB`, `en-US` `` |
| `MessageKey` | `MessageKey::new("setup.step.channel")`, a `const fn` that panics (a compile error in a `const`) unless the key is `.`-separated non-empty ASCII `[A-Za-z0-9_-]` segments; `as_str()`; `is_framework()` for the reserved `noctis.` namespace |
| `Translate` | `fn translate(&self, Locale, MessageKey, &[(&str, &dyn Display)]) -> Option<String>`; object-safe, `Send + Sync`; implementations fall back to `sibling()` then the default locale and replace every `{name}` |

`Locale` variants and codes, in Discord's order: `Id` id, `Da` da, `De` de, `EnGb`
en-GB, `EnUs` en-US, `EsEs` es-ES, `Es419` es-419, `Fr` fr, `Hr` hr, `It` it, `Lt` lt,
`Hu` hu, `Nl` nl, `No` no, `Pl` pl, `PtBr` pt-BR, `Ro` ro, `Fi` fi, `SvSe` sv-SE, `Vi`
vi, `Tr` tr, `Cs` cs, `El` el, `Bg` bg, `Ru` ru, `Uk` uk, `Hi` hi, `Th` th, `ZhCn`
zh-CN, `Ja` ja, `ZhTw` zh-TW, `Ko` ko. A bare language (`en`, `zh`), another region
(`es-MX`), a different case (`en-us`) or separator (`en_US`) and surrounding spaces do
not parse. `sibling()` pairs only `EnUs` ↔ `EnGb` and `EsEs` ↔ `Es419`; every other
locale returns `None`.

## Commands

| Id | Limit | Where |
| --- | --- | --- |
| CMD-01 | name word regex `^[-_ʼ\p{L}\p{N}\p{sc=Deva}\p{sc=Thai}]{1,32}$` (`ʼ` U+02BC, not `'`) | C / B |
| CMD-02 | 1–32 chars per word, no empty words | C / B |
| CMD-03 | lowercase | C / B |
| CMD-04 | description 1–100 | C / B |
| CMD-05 | ≤ 8000 chars per command incl. options, choices, longest localization per field [U] | C / B |
| CMD-06 | unique option names per level | C / B |
| CMD-07 | `nsfw` consistent per base (unset counts) | C / B |
| CMD-10 | ≤ 3 words (command → group → subcommand) | C |
| CMD-11 | a name can't be both subcommand and group | C / B |
| CMD-12 | a base with children can't be invoked or have options | C / B |
| CMD-13 | ≤ 25 options / subcommands / groups per level | C / B |
| CMD-14 | groups and subcommands may coexist | — |
| CMD-15 | pattern ≤ 25 expansions (`max_expansions`), no duplicates | C / B |
| CMD-16 | pattern syntax: whole-word groups, single-word alternatives, `?` only after a group | C / B |
| REG-01 | 100 global chat-input, 15 user, 15 message commands | B |
| REG-02 | same per guild | B / R |
| REG-03 | unique names per type per scope (chat-input merges) | B |
| REG-04 | merged trees must not conflict | B |
| REG-05 | 200 creates / day / guild (Publish is diff-based) | R |
| REG-06 | ≤ 100 permission overwrites per command per guild; needs a Bearer token (out of scope in v1) | — |
| CTX-01 | context-menu name 1–32, mixed case/spaces ok | C / B |
| CTX-02 | context menus: no description, no options | C |
| META-01..05 | permissions, contexts, integration types per base; guild commands can't set contexts/integration types | C / B / R |

## Options

| Id | Limit | Where |
| --- | --- | --- |
| OPT-01 | required before optional | C |
| OPT-03 | INTEGER ±(2^53 − 1) | C / R |
| OPT-04 | NUMBER ±2^53 | C |
| OPT-05 | `min ≤ max`, numbers only | C |
| OPT-06 | `min_length` 0–6000, `max_length` 1–6000, strings only | C |
| OPT-07 | `channel_types` on channels only | C |
| OPT-08 | ≤ 25 choices | C |
| OPT-09 | choice name 1–100, string value ≤ 100 | C / B |
| OPT-10 | choices xor autocomplete | C |
| OPT-11 | autocomplete on string/integer/number only | C |
| OPT-12 | autocomplete input is unvalidated (`Partial<O>`: every field `Option`, lenient decode) | C / R |
| OPT-13 | `file_types` ≤ 10, attachment only; each `image`/`video`/`audio` or `.ext` | C |
| INT-07 | ≤ 25 autocomplete choices (always truncated with a warning; invalid kept choices dropped) | R |

## Localization

| Id | Limit | Where |
| --- | --- | --- |
| LOC-01 | 32 Discord locales (file names) | C |
| LOC-02 | localized values follow CMD/OPT rules | C / B |
| LOC-03 | localized option names unique per command per locale | C / B |
| LOC-04 | partial localization allowed (fallback + warning) | — |
| LOC-05 | longest localization counts toward 8000 | C / B |
| LOC-06 | dispatch uses default names | R |

## Interactions

| Id | Limit | Where |
| --- | --- | --- |
| INT-01 | initial response within 3 s (auto-defer at 2 s) | R |
| INT-02 | token valid 15 min (`TokenExpired`) | R |
| INT-03 | exactly one initial callback (typestate) | C / R |
| INT-04 | callback types per interaction kind (context types) | C |
| INT-05 | modal must be first response; no modal after modal submit | C / R |
| INT-06 | no defer for autocomplete (`respond` only; 2.5 s deadline → empty list) | C / R |
| INT-08 | deferred command reply: only `EPHEMERAL`; V2 flag on edit | C |
| INT-09 | callback flags: SUPPRESS_EMBEDS, EPHEMERAL, IS_COMPONENTS_V2, IS_VOICE_MESSAGE, SUPPRESS_NOTIFICATIONS | C |
| INT-10 | ephemerality fixed at creation | C |
| INT-11 | ≤ 5 follow-ups for user-installed apps outside the guild | R |
| INT-12 | Noctis always edits the original explicitly after a defer | R |
| INT-13 | interaction endpoints exempt from the global HTTP limit | R |

## `custom_id` and Routes

| Id | Limit | Where |
| --- | --- | --- |
| CID-01 | 1–100 chars; proven per Route at compile time | C |
| CID-02 | unique per message | C / R |
| CID-03 | Route id `[a-z0-9._-]{1,24}`, `_` prefix reserved | C |
| CID-04 | one handler per Route; components only for Routes with a handler | C / B |
| CID-05 | Ref = 11 base62 chars, CSPRNG ≥ 64 bits, App-namespaced | C / R |
| CID-06 | `custom_id` is untrusted client input | R |
| CID-07 | ASCII-only encoding | C |

Layout widths: `F` 1, route ≤ 24, `:` 1, schema 1, select nonce 2, Ref marker + Ref
12, integrity tag 6. Field widths: `bool` 1, `u8` 2, `u16` 3, `u32` 6, `u64`/`Id` 11,
`PayloadEnum` 1, `Option<T>` +1, `BoundedStr<N>` 2N, `String`/`Vec` unbounded. Worst
by-ref id: 41 (`MAX_REF_ID_LEN`: 24-character route, schema and select nonce).

Page sizing: `Budget::of(&fixed_parts, ctx.limits()).fits(&row)` answers "how many rows
of this shape still fit" against CV2-03 (`limits.components`, 40) and CV2-04
(`limits.text_total_len`, 4000) together (`views-and-recipes.md` §4):

- `components_left = limits.components - cost(fixed).components` and
  `text_left = limits.text_total_len - cost(fixed).text_chars`, both saturating at 0;
- `fits(&row) = min(components_left / c, text_left / t)` for a row of cost `(c, t)`
  (`t = 0` does not bind); `fits_prefix(rows)` adds rows in order and stops at the
  first that does not fit;
- budgets only: per-field Limits and CID-02 still apply to every row. Measure the fixed
  parts with their variable text at its longest (the page line of the last page).

## Components V2 (messages)

| Id | Limit | Where |
| --- | --- | --- |
| CV2-01 | `IS_COMPONENTS_V2` = 32768 on every send/edit; can't be removed | C |
| CV2-02 | no `content`, embeds, poll, stickers with V2 | C |
| CV2-03 | ≤ 40 components total, all depths (options and gallery items don't count); `components!` counts literal trees | C / R |
| CV2-04 | ≤ 4000 chars across all Text Displays [U]; `components!` sums literal `text("…")` | C / R |
| CV2-05 | placement (no Container in Container; Thumbnail only as accessory; Button only in row/accessory) | C |
| CV2-06 | Action Row: 1–5 buttons or exactly 1 select (`ActionRow::buttons` arity `RowButtons`; `ActionRow::try_buttons` at runtime) | C / R |
| CV2-07 | Section: 1–3 Text Displays + 1 accessory (`Section::new` arity `SectionTexts`; `Section::try_new` at runtime) | C / R |
| CV2-08 | button label ≤ 80 (`label!`, `Label::try_new`); link url ≤ 512; premium = sku only | C / R |
| CV2-09 | select placeholder ≤ 150 (`placeholder!`, `Placeholder::try_new`); min 0–25; max 1–25; ≤ 25 options; option label/value/description ≤ 100; defaults within [min,max]; `disabled` messages only | C / R |
| CV2-10 | Media Gallery 1–10 items (`MediaGallery::new` arity `GalleryItems`; `MediaGallery::try_new` at runtime); description ≤ 1024 | C / R |
| CV2-11 | Thumbnail description ≤ 1024; images only | C / R |
| CV2-12 | File: `attachment://` only | C / R |
| CV2-13 | Separator spacing 1 or 2; accent 0x000000–0xFFFFFF (`rgb!`, `Rgb::try_from(u32)`) | C / R |
| CV2-14 | numeric `id` 32-bit, unique (assigned by Noctis) | C |
| CV2-15 | legacy: ≤ 5 Action Rows, types 1–8 | C |
| CV2-16 | unknown component types round-trip as raw JSON | R |
| CV2-17 | media URL (Media Gallery item, Thumbnail) ≤ 2048 [U] | C / R |

## Modals

| Id | Limit | Where |
| --- | --- | --- |
| MOD-01 | title ≤ 45 (`derive(Modal)` literal at `cargo check`; `noctis::ui::validate` on a `Modal`, Lenient truncates) | C / R |
| MOD-02 | 1–5 top-level (`ModalLabel` or `TextDisplay`; `derive(Modal)` counts fields, one component attribute each) | C / R |
| MOD-03 | no buttons/sections/containers/galleries/Action Rows/message selects (`ModalTopLevel` / `ModalInput`, `[MOD-03]`); modal selects have no `disabled` | C |
| MOD-04 | `ModalLabel` text 1–45, description ≤ 100 | C / B / R |
| MOD-05 | Text Input min 0–4000, max 1–4000, value ≤ 4000, placeholder ≤ 100 | C / R |
| MOD-06 | File Upload min 0–10 (0 only when not required), max 1–10, `file_types` ≤ 10 | C / R |
| MOD-07 | Radio 2–10 options; Checkbox Group 1–10, min 0–10, max 1–10; Checkbox not required (`Checkbox` has no `required`); option label/value 1–100, description ≤ 100 (`derive(Choices)` texts at `cargo check`; counts via `RadioOptions` / `CheckboxOptions` on `Choices::Count`) | C / R |
| MOD-08 | modal `custom_id` 1–100 (the modal struct's, via `#[derive(Modal)]` and `.build()`); input keys 1–100 (CID-01, runtime) | C / R |
| MOD-09 | submitted values untrusted (`ModalForm::from_json` → typed `SubmitError`, never a panic); upload URLs ephemeral (`ResolvedAttachment::download`: CDN host, `attachment_bytes`, length → `DownloadError`) | R |

## Messages, attachments, webhooks

| Id | Limit | Where |
| --- | --- | --- |
| MSG-01 | legacy `content` ≤ 2000 [U] (`content!`, `Content::try_new`) | C / R |
| MSG-02 | embeds (legacy only): ≤ 10; title 256, description 4096, 25 fields, 6000 total | C / R |
| MSG-03 | allowed mentions: `parse` xor explicit ids per type | C |
| MSG-04 | ≤ 100 user ids, ≤ 100 role ids | R |
| MSG-05 | Views default to `parse: []`; escape user text with `escape_markdown` / `escape_mentions` | C |
| MSG-06 | nonce ≤ 25; ≤ 3 stickers | R |
| MSG-07 | flags per endpoint | C |
| MSG-08 | edits replace components; attachments list rebuilt | R |
| MSG-09 | a message is never empty: V2 needs ≥ 1 component; legacy needs non-blank content or ≥ 1 row | R |
| ATT-01 | ≤ 20 MiB per file default; interaction `attachment_size_limit` wins | R |
| ATT-02 | ≤ 25 MiB per request | R |
| ATT-03 | ≤ 10 attachments per message [U] | R |
| ATT-04 | `files[n]` naming (automatic) | R |
| ATT-05 | `attachment://` must reference an attached file | C / R |
| ATT-06 | alt text ≤ 1024 | C / R |
| ATT-07 | image formats png, jpg/jpeg, webp, gif | C |
| WH-01 | `with_components=true` (automatic) | C |
| WH-02 | foreign webhooks: non-interactive only | C |
| WH-03 | V2 webhook body: no content/embeds/files/poll | C |
| WH-04 | webhook name 1–80 | C / R |
| WH-05 | forum/media channels need thread id or name | R |

## Gateway and HTTP

| Id | Limit | Where |
| --- | --- | --- |
| GW-01 | identify buckets by `max_concurrency`, 1 per bucket per 5 s | B / R |
| GW-02 | daily identify budget (1000+) | B / R |
| GW-03 | ≤ 2500 guilds per shard | B |
| GW-04 | 120 gateway sends / 60 s / shard | R |
| GW-05 | privileged intents enabled in the portal and requested | B |
| GW-06 | `large_threshold` 50–250 | C |
| GW-07 | request members: one guild, ≤ 100 user ids | C / R |
| RL-01 | per-route buckets, obey `Retry-After` | R |
| RL-02 | 50 req/s global per token across processes → shared proxy | startup / R |
| RL-03 | 10,000 invalid requests / 10 min → Cloudflare ban | R |
| RL-04 | undocumented per-resource limits (e.g. channel renames) | R |

## Rendering (Noctis defaults, FR-70)

| Limit | Default |
| --- | --- |
| pixels per job / per frame | 4 MP / 1 MP |
| frames / duration | 90 / 6 s |
| fps | GIF ≤ 50, WebP/APNG ≤ 90 |
| size target | ≈ 2 MB (and ≤ Upload Budget) |
| auto-defer deadline | 2000 ms |
| Ref TTL | 24 h |
| ModuleStateStore poll | 5 s |
