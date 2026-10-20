# Modals

Covers: `#[derive(Modal)]` and `#[derive(Choices)]`, typed submissions
(`ModalForm::from_json`, `ModalSubmission`, resolved entities, the File Upload download
helper), the runtime `Modal` builder, Labels, every modal component (Text Input, selects,
File Upload, Radio Group, Checkbox Group, Checkbox, Text Display), pre-fill, opening a
modal, modal-based Wizard Steps. Rules: MOD-01..09, INT-05, CV2-04, CV2-09, CID-01, FR-29,
FR-63, FR-64.

## 1. Anatomy: `#[derive(Modal)]`

A modal form is a struct plus the Route its submission goes to. `derive(Modal)` (in
`noctis::prelude`) implements `noctis::ui::ModalForm` for it:

```rust
use noctis::prelude::*;                    // Route, Modal, Choices, ModalForm, …
use noctis::ui::{ResolvedAttachment, SubmitError};

#[derive(Route, Debug, Clone, Copy)]
#[route("mod.report")]                     // the submit Route (MOD-08: same custom_id rules)
pub struct Report { pub id: u64 }

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    #[choice("Low", value = "l")]
    Low,
    #[choice("High", value = "h", description = "Someone is in danger")]
    High,
}

#[derive(Modal, Debug)]
#[modal(route = Report, title = "Report a user")]          // title 1–45 (MOD-01)
pub struct ReportForm {
    #[text_display("Tell us what happened.")]
    _intro: (),                                              // Text Display, submits nothing
    #[text_input("Reason", style = paragraph, max_length = 500)]
    pub reason: String,                                      // required
    #[radio_group("Severity")]
    pub severity: Severity,                                  // derive(Choices)
    #[file_upload("Evidence", min_values = 0, max_values = 3, file_types = ["image", ".pdf"])]
    pub files: Vec<ResolvedAttachment>,
    #[checkbox("I agree")]
    pub agree: bool,
}

let modal: noctis::ui::Modal = ReportForm::modal(Report { id: 7 });  // ModalForm::modal
let form: ReportForm = ReportForm::from_json(raw_interaction_bytes)?; // ModalForm::from_json
```

- `#[modal(route = R, title = "…")]` is required: `R` is the submit Route (it needs a
  `#[handler]`, else `[CID-04]`), the title a literal.
- **One field = one top-level component**, in declaration order; 1–5 fields (MOD-02),
  counted at `cargo check`. Each field carries **exactly one** component attribute: none
  is `[MOD-02] field … is not a modal component`, two are `[MOD-03] … has two component
  attributes`. A tuple/unit struct, an enum or generics are `[MOD-02]` errors.
- The **field name is the input key** (its `custom_id`, `r#` stripped: `r#type` → `"type"`;
  1–100 characters, CID-01). The modal's own `custom_id` is the Route's.
- The first literal of each attribute is the **Label text** (1–45, MOD-04); every Label
  attribute also takes `description = "…"` (≤ 100). `#[text_display("…")]` is a top-level
  Text Display instead (its literals together ≤ 4000, CV2-04).
- Every text is a **literal**, checked at `cargo check` with the same
  `noctis::rules::modal::validate_modal` the runtime validator uses; errors are spanned on
  the offending literal and cite the rule id (`[MOD-01] title: limit between 1 and 45,
  actual 46`). Localized text (`title(key = …)`, `key = …`, a constant) is rejected with
  `localized modal text is [planned, Epic 11]; use a literal`; for dynamic text build a
  `Modal` at runtime (§4b).
- `Option<_>` field ⇒ optional (`"required": false`); a plain type ⇒ required; a `Vec<_>`
  ⇒ required unless `min_values = 0` (then `"min_values": 0, "required": false`).
  `min_values` / `max_values` are allowed on `Vec` fields only.
- Top level is a **Label** (wrapping one input) or a **Text Display**; Noctis never emits
  the deprecated Action Row + Text Input `label` form. No `disabled`, Buttons, Sections,
  Containers or galleries exist in a modal (MOD-03).

### 1.1 Field attributes and types

| Attribute | Extra arguments | Field types | JSON / defaults |
| --- | --- | --- | --- |
| `#[text_input("L", …)]` | `style = short \| paragraph` (default `short`), `min_length`, `max_length`, `placeholder` | `String`, `Option<String>` | type 4 (MOD-05) |
| `#[string_select("L", …)]` | `placeholder`, `min_values`, `max_values` | `C`, `Option<C>`, `Vec<C>` with `C: Choices` | type 3; options from `C`; a `Vec` without `max_values` gets `C`'s option count (CV2-09) |
| `#[user_select("L", …)]` | `placeholder`, `min_values`, `max_values` | `ResolvedUser`, `Option<_>`, `Vec<_>` | type 5; a `Vec` without `max_values` gets 25 |
| `#[role_select("L", …)]` | same | `ResolvedRole`, `Option<_>`, `Vec<_>` | type 6 |
| `#[mentionable_select("L", …)]` | same | `Mentionable`, `Option<_>`, `Vec<_>` | type 7 |
| `#[channel_select("L", …)]` | same, plus `channel_types = [GuildText, GuildAnnouncement, …]` (`ChannelType` variant names) | `ResolvedChannel`, `Option<_>`, `Vec<_>` | type 8 |
| `#[file_upload("L", …)]` | `min_values`, `max_values`, `file_types = ["image", ".pdf"]` | `ResolvedAttachment`, `Option<_>`, `Vec<_>` | type 19; a `Vec` without `max_values` gets 10 (MOD-06) |
| `#[radio_group("L")]` | — | `C`, `Option<C>` | type 21; 2–10 options (MOD-07) |
| `#[checkbox_group("L", …)]` | `min_values`, `max_values` | `Vec<C>` | type 22; 1–10 options; no `max_values` = option count (MOD-07) |
| `#[checkbox("L")]` | — | `bool` | type 23; never required |
| `#[text_display("content")]` | — (no Label) | `()` | type 10 |

A field type the attribute does not take fails with E0277 ``` `String` cannot be a
`#[checkbox]` field ``` and a note listing the allowed types.

### 1.2 Option sets: `#[derive(Choices)]`

```rust
#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    #[choice("Rust")]                                         // value = "Rust" (the variant name)
    Rust,
    #[choice("Art", value = "art", description = "Drawing, painting")]
    Art,
}
```

- A non-generic enum of **unit variants**; each takes `#[choice("Label")]` or
  `#[choice("Label", value = "v", description = "d")]`. Label and value 1–100,
  description ≤ 100, checked at `cargo check`. A long label or value is reported as
  OPT-09 (the same limit; slash-command choices share the derive): `` [OPT-09] Rust: limit
  between 1 and 100, actual 101 `` with `= note: in the choice name` (or `in the choice
  value`, `limit at most 100`) and `= note: the same limit applies to modal and select
  options (MOD-07, CV2-09)`. An empty `value = ""` and a long description stay
  `[MOD-07]`. `value` defaults to the variant name (`r#` stripped); two variants with one
  value are an error (`duplicate choice value`). Zero variants, a variant with fields or
  generics are `[MOD-07]` errors.
- The derive also implements `noctis::CommandOption`, so the same enum is a STRING slash
  command option with choices (`commands.md` §3).
- It implements `noctis::ui::Choices` (also in the prelude): `Choices::ALL` (every
  variant, in order), `.label()`, `.value()`, `.description()`, `Choices::from_value(&str)
  -> Option<Self>`, and `type Count = Arity<N>` (`N` = the variant count).
- The option count is checked through `Count` at `cargo check`: a `#[radio_group]` needs
  2–10 variants (`[MOD-07] a Radio Group takes 2 to 10 options`, bound `RadioOptions`),
  a `#[checkbox_group]` 1–10 (`CheckboxOptions`), a `#[string_select]` 1–25
  (`[CV2-09] a String Select takes 1 to 25 options`, `StringSelectOptions`). Option lists
  built at runtime use the builders (`RadioGroup::new(key, options)`, §4) and `validate`.
- Option counts versus `min_values` / `max_values` are checked at runtime by
  `noctis::ui::validate` on the built modal.

## 2. Opening it

A Command opens a modal today (Story 6.6): declare `#[command(.., opens_modal)]`, take
`r: CommandResponder<OpensModal>` and return `r.modal(m).await?.done()`:

```rust
#[command("bio", description = "Edit your profile bio", opens_modal, contexts = [Guild])]
async fn bio(ctx: CommandCtx, r: CommandResponder<OpensModal>) -> HandlerResult<Responded> {
    Ok(r.modal(BioForm::modal(SaveBio { user: ctx.user_id() })).await?.done())
}
```

From a component, the **opening** Route must be declared `opens_modal` (no auto-defer,
INT-05) [planned, Story 7.8]:

```rust
#[derive(Route, Debug, Clone, Copy)]
#[route("profile.bio", opens_modal, owner = user)]
pub struct EditBio { pub user: Id<UserMarker> }

#[handler]
async fn edit_bio(ctx: ComponentCtx, e: EditBio, r: ComponentResponder<OpensModal>) -> HandlerResult<Responded> {
    let stored: StoredProfile = ctx.kv().get(&key).await?.unwrap_or_default();
    let modal = BioForm::modal(SaveBio { user: e.user })   // ModalForm::modal → noctis::ui::Modal
        .bio(stored.bio)                                     // pre-fill setters [planned, Story 9.4]
        .pronouns(stored.pronouns);
    Ok(r.modal(modal).await?.done())
}
```

The `OpensModal` responder is required exactly when `opens_modal` is declared (an
`[INT-05]` compile error otherwise), and no auto-defer is armed for that handler; one
that has not answered within 2.9 s logs an error naming it. `modal(m)` after another
initial response, or over the `Dispatcher`, is `ResponderError::ModalUnavailable`.
Do your I/O fast: a modal must be the first response within 3 s.

Today a derived modal has no pre-fill; build the `Modal` at runtime (§4b, `TextInput::value`,
`.default_users(..)`, `ModalOption::default(true)`) when you need one. Pre-filled values go
through the same validator before sending; a value that breaks a field limit is a typed
error (FR-63).

## 3. Receiving it: typed submissions

The `#[handler]` side (`ModalSubmitCtx`, the submit Route and the form as parameters) is
[planned, Story 7.8]:

```rust
#[handler]
async fn save_bio(ctx: ModalSubmitCtx, save: SaveBio, form: BioForm) -> HandlerResult<ProfileView> {
    …
    ProfileView::load(&ctx, save.user).await
}
```

Decoding works today, from the raw interaction JSON (`&[u8]` of the whole interaction
object, `"type": 5`):

```rust
use noctis::ui::{ModalForm, ModalSubmission, SubmitError, SubmittedValue};

let form = ReportForm::from_json(bytes)?;                   // = from_submission(&ModalSubmission::from_slice(bytes)?)
let submission = ModalSubmission::from_slice(bytes)?;
submission.custom_id();                                      // the modal's custom_id (the Route's encoding)
submission.get("reason");                                    // Option<&SubmittedValue>
submission.resolved();                                       // &twilight InteractionDataResolved
submission.unknown();                                        // &[serde_json::Value]: components Noctis does not know
```

- Values decode from the raw interaction JSON, Label-wrapped (type 18) or legacy
  Action-Row-wrapped (type 1); Radio Group, Checkbox Group and Checkbox work although
  twilight-model drops them (FR-64). Unknown components are kept verbatim in `unknown()`,
  never dropped. Decoding never compares `custom_id` with the Route (dispatch is core's).
- `SubmittedValue` (`#[non_exhaustive]`): `TextInput(String)`, `StringSelect(Vec<String>)`,
  `UserSelect(Vec<Id<UserMarker>>)`, `RoleSelect(Vec<Id<RoleMarker>>)`,
  `MentionableSelect(Vec<Id<GenericMarker>>)`, `ChannelSelect(Vec<Id<ChannelMarker>>)`,
  `FileUpload(Vec<Id<AttachmentMarker>>)`, `RadioGroup(Option<String>)`,
  `CheckboxGroup(Vec<String>)`, `Checkbox(bool)`.
- Per field: an absent input, or an empty one on a single-valued field, is `None` for
  `Option`, else `SubmitError::Missing`; an absent or empty input is `vec![]` for a
  non-required `Vec`, while an empty required `Vec` is `Invalid` (count below its minimum)
  and an absent one `Missing`. Text `""` counts as empty; a text outside the declared
  `min_length..=max_length` (by `discord_len`) is `Invalid`. A single-valued field with 2+
  values, a `Vec` outside its effective `min..=max` (min = `min_values`, else 1 when
  required) and an unknown choice value are `Invalid`. Selects join `resolved`: users (with
  `member` when the submission has one), roles, channels; Mentionable tries the user, then
  the role; uploads join `resolved.attachments`. A missing entry is `Unresolved`.
- `SubmitError` (`#[non_exhaustive]`, Display starts `[MOD-09]`): `Malformed { reason }`
  (bad JSON, missing/ill-typed `type`/`data`/`custom_id`/`components`, an input without a
  string `custom_id`, bad `resolved`), `NotModalSubmit { kind }`, `Duplicate { key }`,
  `Missing { key }`, `WrongType { key, expected, found }` (Discord component type numbers),
  `Invalid { key, reason }`, `Unresolved { key, id }`. Decoding never panics.
- Resolved types (`#[non_exhaustive]`, pub fields): `ResolvedUser { user, member:
  Option<InteractionMember> }`, `ResolvedRole { role }`, `ResolvedChannel { channel:
  InteractionChannel }`, `Mentionable::{User(ResolvedUser), Role(ResolvedRole)}`,
  `ResolvedAttachment { attachment }` with `.id()`, `.filename()`, `.size()`,
  `.content_type()`, `.url()`. Build one from resolved data by hand with
  `ResolvedUser::from_resolved(&resolved, id)` (member attached when present),
  `ResolvedRole::from_resolved`, `ResolvedChannel::from_resolved`,
  `ResolvedAttachment::from_resolved` and `Mentionable::from_resolved(&resolved,
  Id<GenericMarker>)` (user first, then role); each returns `None` when the id is missing.
- Submitted values are untrusted (MOD-09): re-validate ranges and authorization.
- If the modal was opened from a message component, returning a View **updates that
  message** (type 7); from a command, it sends a new message. `ModalResponder` has no
  `modal()`: modals can't chain (INT-05). [planned, Story 7.8]
- Per-field validation [planned]: `#[validate(fn)]` → `fn(&T) -> Result<(), MessageKey>`.

### 3.1 Downloading uploads

A `ResolvedAttachment`'s URL is an **ephemeral**, signed CDN URL that expires: download
promptly and re-check the content type (MOD-09).

```rust
use noctis::rules::Limits;
use noctis::ui::{DownloadError, FileFetcher};

let bytes = form.files[0].download(&fetcher, &limits).await?;   // -> Result<Bytes, DownloadError>
```

- `fetcher: &impl FileFetcher` (`fn fetch(&self, url: &str, max_bytes: u64) -> impl
  Future<Output = Result<Bytes, Box<dyn Error + Send + Sync>>> + Send`); core supplies the
  HTTP implementation [planned, Epic 7], tests pass a fake. A fetcher must fail (never
  truncate) on a body over `max_bytes`, never read beyond it and never follow a redirect off
  the two CDN hosts.
- Only `https://cdn.discordapp.com/` and `https://media.discordapp.net/` URLs are fetched
  (else `DownloadError::Host`), a file over `limits.attachment_bytes` is refused
  (`TooLarge`), neither calls the fetcher; a fetched length other than `size()` is
  `SizeMismatch`, a fetcher failure `Fetch(source)`.

## 4. Components

Every modal component has its own builder in `noctis::ui`; the `derive(Modal)` field
attributes (§1.1) build these same types. Each input takes a string **key** first: it is the input's `custom_id` and names the
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
with `ResolvedAttachment::download` (§3.1) and re-check the content type (MOD-09).

**Builder:** `FileUpload::new(key)`, `.min_values(u8)`, `.max_values(u8)`,
`.required(bool)`, `.file_types(["image", ".pdf"])` (appends). JSON:
`{"type":19,"custom_id","min_values"?,"max_values"?,"required":false?,"file_types"?}`.

### 4.9 Radio Group: `RadioGroup`

**Where:** inside a `ModalLabel` only. Type 21. Submits one option or none.

**Limits:** 2–10 options; each `ModalOption` label and value 1–100, description ≤ 100
(MOD-07). The constructor accepts any count; `validate` rejects a wrong one. A
`#[radio_group]` field's option count is checked at `cargo check` instead (§1.2).

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
and counts toward the 5. In a derived modal it is a `#[text_display("…")]` field of type
`()`.

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
  input key in order, then CID-02 for each repeated key (note `first used at <path>`),
  then CV2-04 at `components` when the Text Displays together exceed 4000 characters.
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
  and descriptions (CV2-09), Radio/Checkbox option labels and descriptions (MOD-07),
  then the Text Displays to fit the CV2-04 total (one warning at `components`).
  Never a Text Input `value`, option values, keys or counts: those still fail. Strict
  mode (the default) never changes the modal.
- `r.modal(modal)` on a `CommandResponder<OpensModal>` runs this validator (and fails
  with `ResponderError::Invalid`) before any request, then sends callback type 9 (Story
  6.6); from components it is [planned, Story 7.8].

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

`derive(Modal)` (at `cargo check`, on its literals) and the runtime validator check modals
with these functions; use them for modals built from dynamic data (localized titles, pre-filled values, generated option
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
