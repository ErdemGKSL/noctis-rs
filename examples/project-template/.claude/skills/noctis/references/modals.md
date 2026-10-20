# Modals

Covers: `#[derive(Modal)]` [planned, Story 4.6], the runtime `Modal` builder, Labels, every
modal component (Text Input, selects, File Upload, Radio Group, Checkbox Group, Checkbox,
Text Display), pre-fill, opening a modal,
typed submissions, modal-based Wizard Steps. Rules: MOD-01..09, INT-05, CV2-09, FR-29,
FR-63, FR-64.

## 1. Anatomy

`#[derive(Modal)]` is [planned, Story 4.6]; today build a `Modal` at runtime (§4b). With
the derive, a modal is a form struct plus the Route its submission goes to:

```rust
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.save", owner = user)]           // submit Route (MOD-08: same custom_id rules)
pub struct SaveBio { pub user: Id<UserMarker> }

#[derive(Modal, Debug)]
#[modal(route = SaveBio, title(key = "profile.bio_modal.title"))]   // title ≤ 45 (MOD-01)
pub struct BioForm {
    #[label(key = "profile.bio_modal.bio")]
    #[text_input(style = paragraph, max_length = 300, placeholder(key = "profile.bio_modal.bio_hint"))]
    pub bio: Option<String>,

    #[label(key = "profile.bio_modal.pronouns")]
    #[radio_group]
    pub pronouns: Option<Pronouns>,              // derive(Choices)
}
```

- One field = one top-level component; 1–5 per modal (MOD-02), counted by the derive.
- Top level is a **Label** (wrapping one input) or a **Text Display**
  (`#[text] intro: Static` with `#[text(key = "…")]`) [planned]. Noctis never emits the
  deprecated Action Row + Text Input `label` form.
- No `disabled` anywhere, no Buttons, Sections, Containers or galleries (MOD-03): the
  modal field types simply don't have them.
- Text: every attribute that takes text accepts a literal (`"Bio"`, length-checked at
  compile time) or `key = "…"` (resolved in the clicking user's locale, checked at App
  build / runtime).
- `Option<T>` field ⇒ `required = false`; non-`Option` ⇒ required.

## 2. Opening it

The **opening** Route must be declared `opens_modal` (no auto-defer, INT-05):

```rust
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.bio", opens_modal, owner = user)]
pub struct EditBio { pub user: Id<UserMarker> }

#[handler]
async fn edit_bio(ctx: ComponentCtx, e: EditBio, r: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    let stored: StoredProfile = ctx.kv().get(&key).await?.unwrap_or_default();
    let modal = BioForm::modal(SaveBio { user: e.user })   // generated BioFormModal builder
        .bio(stored.bio)                                     // pre-fill (FR-63), one setter per field
        .pronouns(stored.pronouns);
    Ok(r.modal(modal).await?.done())
}
```

Commands can open modals too: `#[command("feedback", opens_modal)]` +
`CommandResponder<OpensModal>`. The `OpensModal` alias is required exactly when
`opens_modal` is declared (INT-05).
Do your I/O fast: a modal must be the first response within 3 s.

Pre-filled values go through the same validator before sending; a value that breaks a
field limit is a typed error (FR-63). Optional builder overrides: `.title(..)` [planned].

## 3. Receiving it

Typed submissions are [planned, Story 4.6].

```rust
#[handler]
async fn save_bio(ctx: ModalSubmitCtx, save: SaveBio, form: BioForm) -> HandlerResult<ProfileView> {
    // `form` is fully typed: selects joined to resolved entities, uploads to attachments.
    …
    ProfileView::load(&ctx, save.user).await
}
```

- If the modal was opened from a message component, returning a View **updates that
  message** (type 7); from a command, it sends a new message.
- `ModalResponder` has no `modal()`: modals can't chain (INT-05).
- Values decode from the raw interaction JSON, so Radio Group, Checkbox Group and
  Checkbox work even where twilight drops them (FR-64). Unknown parts are kept as raw JSON.
- Submitted values are untrusted (MOD-09): re-validate ranges and authorization.
- Per-field validation [planned]: `#[validate(fn)]` → `fn(&T) -> Result<(), MessageKey>`;
  a failure replies with a localized ephemeral message and the handler doesn't run.

## 4. Components

Every modal component has its own builder in `noctis::ui`. The `#[label]`, `#[text_input]`,
… field attributes of `derive(Modal)` are [planned, Story 4.6]; they build these same
types. Each input takes a string **key** first: it is the input's `custom_id` and names the
submitted field (1–100 characters, unique within the modal; CID-01, CID-02). The key is
never a Route; the modal's own `custom_id` is.

### 4.1 Label: `ModalLabel`

**Where:** the top level of a `Modal` (`ModalTopLevel`), wrapping exactly one input
(`ModalInput`). Type 18. It is Discord's Label component; `noctis::ui::Label` is the
unrelated checked button-label text.

**Limits:** `label` 1–45, `description` ≤ 100 (MOD-04). A Label and its input cost 2
components. Anything but the ten modal inputs fails `cargo check` with `[MOD-03] … cannot
be a modal Label input`.

**Builder:** `ModalLabel::new(label, input)`, `.description(..)`. JSON:
`{"type":18,"label","description"?,"component":{…}}`.

### 4.2 Text Input: `TextInput`

**Where:** inside a `ModalLabel` only. Type 4. Never wrapped in an Action Row and never
with a `label` of its own (the Label holds the text).

**Limits:** `min_length` 0–4000, `max_length` 1–4000, `min_length ≤ max_length`, `value`
(pre-fill) ≤ 4000, `placeholder` ≤ 100 (MOD-05). Lenient mode truncates the placeholder,
never the `value`.

**Builder:** `TextInput::new(key, TextInputStyle::Short | Paragraph)`, `.min_length(u16)`,
`.max_length(u16)`, `.required(bool)` (default `true`), `.value(..)` (pre-fill, FR-63),
`.placeholder(..)`. JSON: `{"type":4,"custom_id","style","min_length"?,"max_length"?,
"required":false?,"value"?,"placeholder"?}`.

### 4.3 Modal String Select: `ModalStringSelect`

**Where:** inside a `ModalLabel` only. Type 3. A message `StringSelect` is rejected with
`[MOD-03]`.

**Limits:** 1–25 `SelectOption`s, `placeholder` ≤ 150, `min_values` 0–25, `max_values`
1–25, `min_values(0)` only with `.required(false)` (CV2-09). No `disabled` method (MOD-03).

**Builder:** `ModalStringSelect::new(key, options)` (options are `SelectOption`s),
`.placeholder(..)`, `.min_values(u8)`, `.max_values(u8)`, `.required(bool)` (default
`true`; `"required":false` is written only when false).

### 4.4 Modal User Select: `ModalUserSelect`

**Where:** inside a `ModalLabel` only. Type 5.

**Limits:** as `ModalStringSelect` without options; `default_values` within
`min_values..=max_values` (CV2-09). No `disabled`.

**Builder:** `ModalUserSelect::new(key)`, `.default_users(ids)`, plus `.placeholder`,
`.min_values`, `.max_values`, `.required`.

### 4.5 Modal Role Select: `ModalRoleSelect`

**Where:** inside a `ModalLabel` only. Type 6.

**Limits:** as `ModalUserSelect` (CV2-09). No `disabled`.

**Builder:** `ModalRoleSelect::new(key)`, `.default_roles(ids)`, plus `.placeholder`,
`.min_values`, `.max_values`, `.required`.

### 4.6 Modal Mentionable Select: `ModalMentionableSelect`

**Where:** inside a `ModalLabel` only. Type 7 (users and roles).

**Limits:** as `ModalUserSelect` (CV2-09). No `disabled`.

**Builder:** `ModalMentionableSelect::new(key)`, `.default_users(ids)`,
`.default_roles(ids)`, plus `.placeholder`, `.min_values`, `.max_values`, `.required`.

### 4.7 Modal Channel Select: `ModalChannelSelect`

**Where:** inside a `ModalLabel` only. Type 8.

**Limits:** as `ModalUserSelect` (CV2-09). No `disabled`.

**Builder:** `ModalChannelSelect::new(key)`, `.default_channels(ids)`,
`.channel_types([ChannelType::GuildText, …])` (appends; none = every type), plus
`.placeholder`, `.min_values`, `.max_values`, `.required`.

### 4.8 File Upload: `FileUpload`

**Where:** inside a `ModalLabel` only. Type 19.

**Limits:** `min_values` 0–10 (0 only with `.required(false)`), `max_values` 1–10,
`min ≤ max`, `file_types` ≤ 10 entries, each `image`, `video`, `audio` or `.ext` (MOD-06).
Size is capped by the user's upload limit. URLs are **ephemeral**: download promptly
[planned, Story 4.6: download helper] and re-check the content type (MOD-09).

**Builder:** `FileUpload::new(key)`, `.min_values(u8)`, `.max_values(u8)`,
`.required(bool)`, `.file_types(["image", ".pdf"])` (appends). JSON:
`{"type":19,"custom_id","min_values"?,"max_values"?,"required":false?,"file_types"?}`.

### 4.9 Radio Group: `RadioGroup`

**Where:** inside a `ModalLabel` only. Type 21. Submits one option or none.

**Limits:** 2–10 options; each `ModalOption` label and value 1–100, description ≤ 100
(MOD-07). The constructor accepts any count; `validate` rejects a wrong one (the
compile-time count from `derive(Choices)` is [planned, Story 4.6]).

**Builder:** `RadioGroup::new(key, [ModalOption::new(label, value), …])`, `.required(bool)`.
`ModalOption::new(label, value)`, `.description(..)`, `.default(bool)`; JSON
`{"value","label","description"?,"default":true?}`.

### 4.10 Checkbox Group: `CheckboxGroup`

**Where:** inside a `ModalLabel` only. Type 22.

**Limits:** 1–10 options, `min_values` 0–10 (0 only with `.required(false)`),
`max_values` 1–10, `min ≤ max` (default max = option count); option text as Radio Group
(MOD-07).

**Builder:** `CheckboxGroup::new(key, options)`, `.min_values(u8)`, `.max_values(u8)`,
`.required(bool)`.

### 4.11 Checkbox: `Checkbox`

**Where:** inside a `ModalLabel` only. Type 23.

**Limits:** never required (MOD-07): it has no `required` method. For "must accept", use a
one-option `CheckboxGroup` with `min_values(1)`.

**Builder:** `Checkbox::new(key)`, `.default(bool)`. JSON:
`{"type":23,"custom_id","default":true?}`.

### 4.12 Text Display at the top level

A `TextDisplay` (type 10, see `components-v2.md` §2) may sit at the top level of a modal
and counts toward the 5. `#[text(key = "…")]` on a unit field is [planned, Story 4.6].

## 4b. Building a modal at runtime: `Modal`

`Modal` is the callback `data` of a modal (type 9). It is built only from a Route value,
so its `custom_id` is that Route's encoding (MOD-08) and the submission goes to that
Route's handler. There is no `Modal::new`, no string id and no `Default`; a Route without
a `#[handler]` fails with `[CID-04]`.

```rust
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    Checkbox, FileUpload, ModalLabel, ModalOption, RadioGroup, TextDisplay, TextInput,
    TextInputStyle, validate,
};

let mut modal = Report { id: 7 }.modal("Report a user")      // RouteComponents::modal
    .push(TextDisplay::new("Tell us what happened."))
    .push(ModalLabel::new("Reason", TextInput::new("reason", TextInputStyle::Paragraph).max_length(500)))
    .push(ModalLabel::new("Severity", RadioGroup::new("sev", [ModalOption::new("Low", "l"), ModalOption::new("High", "h")])))
    .push(ModalLabel::new("Evidence", FileUpload::new("files").required(false).min_values(0).max_values(3)))
    .push(ModalLabel::new("I agree", Checkbox::new("ok")));
validate(&mut modal, &Limits::default())?;                     // CostReport { components: 9, … }
```

- `push` takes a `ModalLabel`, a `TextDisplay` or a `ModalComponent` (either, for lists
  built at runtime; `ModalComponent::from(label)`). Anything else fails `cargo check` with
  `[MOD-03] … cannot be a top-level modal component`. Also `title()`, `len()`,
  `is_empty()`.
- JSON: `{"custom_id","title","components":[…]}`, no numeric `id` anywhere, never an
  Action Row (type 1) and never a Text Input `label`. A Route whose `custom_id` needs a
  View envelope, select nonce or signature is resolved by the Responder before sending;
  serializing it unresolved is a serde error naming the Route.
- `noctis::ui::validate(&mut modal, &limits)` returns a `CostReport` or
  `UiError::Limits { violations }`. Order: the `noctis::rules::modal::validate_modal`
  violations (MOD-01 title, MOD-02 count, then per component; §7), then CID-01 for every
  input key in order, then CID-02 for each repeated key (note `first used at <path>`).
- Paths: `title`, `components`, `label[i].label`, `label[i].description`,
  `label[i].text_input.max_length`, `label[i].select.options[k].label`,
  `label[i].file_upload.file_types[k]`, `label[i].radio_group.options`,
  `label[i].checkbox.custom_id`, … (`i` counts every top-level component, Text Displays
  included).
- `noctis::ui::cost(&modal, &limits)`: `components` = top-level count + one per Label
  input, `text_chars` = the Text Display characters; never changes the modal.
- Lenient mode (`limits.mode = LimitMode::Lenient`) first truncates with `…`, each a
  `LimitWarning::Truncated`: the title (MOD-01), Label text and description (MOD-04),
  Text Input placeholders (MOD-05), select placeholders and String Select option labels
  and descriptions (CV2-09), Radio/Checkbox option labels and descriptions (MOD-07).
  Never a Text Input `value`, option values, keys or counts: those still fail. Strict
  mode (the default) never changes the modal.
- Sending it (`r.modal(modal)` on an `OpensModal` responder) is core's [planned, Story 7.8].

## 5. Modals in Wizards

A Step that needs free text shows a button (an `opens_modal` Route carrying the
Wizard state). The modal's submit Route also carries the state; its handler returns
`MyWizard::new(new_state).at(Nav::step(k + 1))`, updating the Wizard message.

## 6. Choosing modal vs message components

- Message selects fire on every change and are visible to everyone on a public message.
- Modals batch several inputs into one atomic submit and collect free text.
- Use a modal for 2+ related inputs, free text, numbers, files; message selects for
  single picks with live feedback.

## 7. Rule functions (`noctis::rules::modal`)

`derive(Modal)` and the runtime validator check modals with these functions; use them for
modals built from dynamic data (localized titles, pre-filled values, generated option
lists). Every function takes `&Limits` and only reports.

```rust
use noctis::rules::Limits;
use noctis::rules::modal::{
    Label, LabelChild, Modal, ModalComponent, ModalOption, RadioGroup, TextInput, TextInputStyle,
    validate_modal,
};

let options = [ModalOption::new("Low", "low"), ModalOption::new("High", "high")];
let components = [
    ModalComponent::Text("Tell us what happened."),
    ModalComponent::Label(Label::new("Reason", LabelChild::TextInput(
        TextInput::new(TextInputStyle::Paragraph).min_length(10).max_length(500),
    ))),
    ModalComponent::Label(Label::new("Severity", LabelChild::RadioGroup(RadioGroup::new(&options)))),
];
validate_modal(&Modal::new(&title, &components), &Limits::default()).map_err(report)?;
```

| Item | What it is |
| --- | --- |
| `Modal<'a>` | `{ title, components: &[ModalComponent] }`; `Modal::new(title, &components)` |
| `ModalComponent<'a>` | `Label(Label)`, `Text(&str)`, or `Message(Component)` (a message component from `noctis::rules::component`; only `Component::Text` is allowed, anything else is MOD-03) |
| `Label<'a>` | `{ label, description: Option<&str>, child: LabelChild }`; `Label::new(text, child)`, `.description(..)` |
| `LabelChild<'a>` | `TextInput(TextInput)`, `Select(ModalSelect)`, `FileUpload(FileUpload)`, `RadioGroup(RadioGroup)`, `CheckboxGroup(CheckboxGroup)`, `Checkbox(Checkbox)`; no button variant exists (MOD-03) |
| `TextInputStyle` | `Short` (1) / `Paragraph` (2), `.value()` |
| `TextInput<'a>` | `{ style, min_length, max_length: Option<usize>, required (default true), value, placeholder: Option<&str> }`; `TextInput::new(style)`, setters `.min_length(n)`, `.max_length(n)`, `.required(bool)`, `.value(..)`, `.placeholder(..)` |
| `ModalSelect<'a>` | `{ select: component::Select, required (default true) }`; `ModalSelect::new(select)`, `.required(bool)` |
| `FileUpload<'a>` | `{ min_values, max_values: Option<usize>, required (default true), file_types: &[&str] }`; `FileUpload::new()` / `Default`, setters |
| `ModalOption<'a>` | `{ label, value, description, default }`; `ModalOption::new(label, value)`, `.description(..)`, `.default(bool)` |
| `RadioGroup<'a>` | `{ options, required (default true) }`; `RadioGroup::new(&options)` |
| `CheckboxGroup<'a>` | `{ options, min_values, max_values, required (default true) }`; `CheckboxGroup::new(&options)`, setters |
| `Checkbox` | `{ default, required (default false) }`; `Checkbox::new()`; `required(true)` is MOD-07 |
| `validate_modal(&Modal, &Limits) -> Result<(), Vec<Violation>>` | the whole modal, every violation at once |
| `validate_modal_title(&str, &Limits) -> Vec<Violation>` | MOD-01 1–45, path `""` |
| `validate_label_text(&str, &Limits)` / `validate_label_description(&str, &Limits)` | MOD-04 1–45 / ≤ 100 |
| `validate_text_input(&TextInput, &Limits)` | MOD-05 on its own; paths `min_length`, `max_length`, `value`, `placeholder`, `""` for the order |
| `validate_text_input_value(&str, &Limits)` | MOD-05 pre-fill ≤ 4000 |
| `validate_input_custom_id(&str, &Limits) -> Vec<Violation>` | CID-01: a modal input key 1–100 (`discord_len`), path `""`; `noctis::ui::validate` re-roots it to `label[i].<input>.custom_id` |

Paths: `title`, `components`, then per top-level component `label[i]`, `text[i]` or
`kind[i]` of a `Message` component; a Label child is `label[i].text_input`, `.select`,
`.file_upload`, `.radio_group`, `.checkbox_group` or `.checkbox`, followed by the field
(`.min_length`, `.min_values`, `.file_types[k]`, `.options[k].label`, `.disabled`,
`.required`, …).

Order: MOD-01 title (1–45), MOD-02 count (1–5), then per component in input order:
MOD-03 for a `Message` (`only Labels or Text Displays at the top level of a modal`, not
walked); for a Label: text (1–45), description (≤ 100), then the child. Text Input:
`min_length` (0–4000), `max_length` (1–4000), `min_length ≤ max_length` (only when both
are set and valid), `value` (≤ 4000), `placeholder` (≤ 100). Select: MOD-03 `no disabled
in a modal`, the CV2-09 select checks (`components-v2.md` §15), then CV2-09 `min_values ≥
1 when required` for `min_values(0)` on a required select. File Upload: `min_values`
(0–10), `max_values` (1–10), order, MOD-06 `min_values ≥ 1 when required`, `file_types`
count (≤ 10), then each entry (`image`, `video`, `audio` or `.ext`, as OPT-13). Radio
Group: 2–10 options, then per option label and value (1–100), description (≤ 100;
`modal_option_text_len`). Checkbox Group: 1–10 options, `min_values` (0–10), `max_values`
(1–10), order, required, then per option. Checkbox: `a Checkbox is never required`.
Effective defaults: selects and File Uploads min 1 / max 1, Checkbox Groups min 1 / max =
option count; `min ≤ max` runs only when both effective values are in range.
