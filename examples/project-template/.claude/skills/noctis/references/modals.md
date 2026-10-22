# Modals

Covers: `#[derive(Modal)]` and `#[derive(Choices)]`, typed submissions
(`ModalForm::from_json`, `ModalSubmission`, resolved entities, the File Upload download
helper), the runtime `Modal` builder, Labels, every modal component (Text Input, selects,
File Upload, Radio Group, Checkbox Group, Checkbox, Text Display), pre-fill, opening a
modal, modals in step flows and the button → modal → re-render chain. Rules: MOD-01..09, INT-05, CV2-04, CV2-09, CID-01, FR-29,
FR-63, FR-64.

## 1. Anatomy: `#[derive(Modal)]`

A modal is one struct, like every interaction: `#[derive(Modal)]`
with `#[modal("id", <routing args>, defaults(title = …))]`. Fields **without** an input
attribute are its Payload (encoded in the modal's `custom_id`, length-proven like any
component, MOD-08); fields **with** one are the inputs. Its handler is a method on it
(§3). `derive(Modal)` (in `noctis::prelude`) implements `noctis::ui::ModalForm`:

```rust
use noctis::prelude::*;                    // Modal, Choices, ModalForm, …
use noctis::ui::{ResolvedAttachment, SubmitError};

#[derive(Choices, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    #[choice("Low", value = "l")]
    Low,
    #[choice("High", value = "h", description = "Someone is in danger")]
    High,
}

#[derive(Modal, Debug)]
#[modal("mod.report", response = ephemeral, defaults(title = keys::MOD_REPORT_TITLE))]  // title 1–45 (MOD-01); a `keys::` path, translated per locale
pub struct ReportForm {
    pub id: u64,                                             // Payload: no input attribute
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

let modal: noctis::ui::Modal = ReportForm::blank(7).into();          // Payload only, inputs empty
let form: ReportForm = ReportForm::from_json(raw_interaction_bytes)?; // ModalForm::from_json
```

`X { .. }.build()` builds from a whole value and returns a `ModalBuilder<X>` (§4a) that
pre-fills every input from its field: a Text Input's `value`, the chosen options marked
`default`, picked users/roles/channels as `default_values`, a `true` Checkbox checked;
`None`, `""`, `vec![]` and `false` send nothing, and File Uploads are never pre-filled.
`X::blank(<payload fields in order>)` builds with empty inputs. It returns `X` itself when
every input field has an empty value (`Option<_>`, `Vec<_>`, `String`, `bool`, the `()` of
a Text Display), so `X { bio: v, ..X::blank(user) }` pre-fills only what you name; a form
with an input that has none (a required `Choices` enum or `ResolvedRole`, a required File
Upload) gets a `ModalBuilder<X>` instead. Both convert into `Modal` (`.into()`,
`Modal::from(..)`) and `show_modal` takes either. Pre-fill never changes the `custom_id`,
the decode or the declared specs.

- `#[modal("id", .., defaults(title = "…"))]` is required: the id is the modal's own
  (it needs a `#[handler]` method, else `[CID-04]`), `defaults(..)` must give `title`
  (a missing block or key is a compile error naming it: ``#[modal]` needs
  `defaults(title = "…")` ``), and an unknown key is an error. The withdrawn
  `#[modal(route = R, title = "…")]` form is a compile error whose help shows this one.
- **One input field = one top-level component**, in declaration order; 1–5 inputs (MOD-02),
  counted at `cargo check`. Each input field carries **exactly one** component attribute
  (a field with none is a Payload field), two are `[MOD-03] … has two component
  attributes`. A tuple/unit struct, an enum or generics are `[MOD-02]` errors.
- The **field name is the input key** (its `custom_id`, `r#` stripped: `r#type` → `"type"`;
  1–100 characters, CID-01). The modal's own `custom_id` is the Route's.
- The first literal of each attribute is the **Label text** (1–45, MOD-04); every Label
  attribute also takes `description = "…"` (≤ 100). `#[text_display("…")]` is a top-level
  Text Display instead (its literals together ≤ 4000, CV2-04).
- Every literal text is checked at `cargo check` with the same
  `noctis::rules::modal::validate_modal` the runtime validator uses; errors are spanned on
  the offending literal and cite the rule id (`[MOD-01] title: limit between 1 and 45,
  actual 46`). The title may be a locale key instead: `defaults(title = keys::T)` is
  resolved in the User Locale first and checked after resolving (MOD-01 at run time).
  Input attribute texts are literals only: a path (`#[text_input(keys::L)]`) is rejected
  with ``[MOD-04] the Label text is a locale key path: input attribute texts are string
  literals; localize them per render with `InputOverrides` ``; localize Labels,
  descriptions and placeholders per render with the `ModalBuilder` overrides (§4a,
  `.overrides(XOverrides { bio: InputOverrides { label: Some(keys::L.into()), .. }, .. })`),
  which take a literal or a `MessageKey`.
- An input field may not be named `title` (the overrides' own field) or a name whose
  upper-case handle is a derived const (`id` → `ID`, `defaults`, `route`, `shape`,
  `schema`, `runtime`, `response`, `guarded`): a compile error names the field.
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

Only a handler declared **`auto_defer = false`** can open a modal, and it must take the
`Manual` responder: a modal must be the first response (INT-05), and an auto-defer would
make it impossible. A Command:

```rust
#[derive(SlashCommand)]
#[command("bio", description = "Edit your profile bio", auto_defer = false, contexts = [Guild])]
pub struct Bio;

impl Bio {
    #[handler]
    async fn handle(self, ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
        let form = BioForm { user: ctx.user_id(), bio: None, pronouns: None };  // empty inputs
        Ok(r.show_modal(form.build()).await?.done())
    }
}
```

**A button that opens a modal must be `auto_defer = false`**, and its handler takes
`ComponentResponder<Manual>`:

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("profile.bio", auto_defer = false, owner = user, defaults(style = Primary, label = keys::PROFILE_EDIT_BIO))]
pub struct EditBio { pub user: Id<UserMarker> }

impl EditBio {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        let stored: StoredProfile = ctx.kv().get(&key).await?.unwrap_or_default();
        // The value's input fields pre-fill the inputs; `None` sends nothing.
        let form = BioForm { bio: stored.bio, pronouns: stored.pronouns, ..BioForm::blank(self.user) };
        Ok(r.show_modal(form.build()).await?.done())
    }
}
```

Per-render overrides on the built modal (§4a): `.title(..)`, `.value(BioForm::BIO, v)`
and `.overrides(BioFormOverrides { .. })`; anything not overridden comes from the
declaration.

The `Manual` responder is required exactly when `auto_defer = false` is declared (an
`[INT-05]` compile error otherwise), `show_modal` on an auto-defer responder fails with
`[INT-05] show_modal needs auto_defer = false on this handler`, and no auto-defer is
armed for that handler; one that has not answered within 2.9 s logs an error naming it.
`show_modal(m)` after another initial response, or over the `Dispatcher`, is
`ResponderError::ModalUnavailable`. Do your I/O fast: a modal must be the first response
within 3 s. The button's type-9 callback carries the modal's `custom_id` (the struct's
encoded Route id with its Payload) and the declared components; the submit of that id
runs the struct's `#[handler]` (§3).

Today a derived modal has no pre-fill; build the `Modal` at runtime (§4b, `TextInput::value`,
`.default_users(..)`, `ModalOption::default(true)`) when you need one. Pre-filled values go
through the same validator before sending; a value that breaks a field limit is a typed
error (FR-63).

## 3. Receiving it: typed submissions

The handler is a method on the modal struct; `self` is the decoded Payload plus the
submitted values, decoded by component dispatch from the raw interaction bytes (Radio
Group, Checkbox Group and Checkbox included). It takes `(self, ctx: ModalSubmitCtx[, r:
ModalResponder])` and returns the View:

```rust
impl BioForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<V2Message> {
        …                                                         // validate self.bio, then save
        let data = load(&ctx, self.user).await?;
        Ok(render_profile(&data, &Strings::of(ctx.response_locale()), true))   // updates the message
    }
}
```

The full flow, end to end: a button declared `auto_defer = false` →
`r.show_modal(Form { .. }.build())` (type 9) → the user submits → `#[handler] async fn
handle(self, ctx: ModalSubmitCtx)` returns the View → it updates the button's message
(type 7). The `/bio` variant opens the same modal from `#[command("bio", ..,
auto_defer = false)]` with `CommandResponder<Manual>` (§2); its submit has no message to
update, so under the default `response = update` a returned View is a new **public**
message (type 4).

### 3.0 `response` and `auto_defer` on `#[modal]`

`#[modal("id", response = update | ephemeral | public, auto_defer = true | false,
defaults(..))]`: the same grammar as the component attributes, in any order, around
`defaults(..)`, the Guards and the codec arguments. Neither changes the `custom_id`.

| Declared | Slow handler (past `ack_deadline`): auto-defer | Returned message, unanswered |
| --- | --- | --- |
| `response = update` (default), opened from a component message | type 6, then `PATCH @original` | type 7: updates that message |
| `response = update`, opened from a slash command (no `message`) | type 5 public, then `PATCH @original` | type 4, public |
| `response = ephemeral` (with or without a source message) | type 5 `flags: 64`, then `PATCH @original` | type 4, `EPHEMERAL` |
| `response = public` (with or without a source message) | type 5, then `PATCH @original` (a new message, never type 6) | type 4, public |
| `auto_defer = false` | none; one `error` naming the handler at 2.9 s | as its `response` |

- `auto_defer = false` requires the handler (and its `#[on_stale]`/`#[on_expired]`
  hooks) to take `ModalResponder<Manual>`, and `ModalResponder<Manual>` requires
  `auto_defer = false`: either mismatch is an `[INT-05]` compile error. It answers
  within 3 s itself (`r.reply(..)`, `r.defer_reply()`, …).
- `response = edit`, `response = "update"` or a bare `response` is ``[CID-03] `response`
  takes `update`, `ephemeral` or `public` ``; `auto_defer = 0` is ``[CID-03] `auto_defer`
  takes `true` or `false` `` (help: it takes `ModalResponder<Manual>`); either given twice
  is ``[CID-03] #[modal] argument `response` is given twice``. `rate_limit(..)` is still
  rejected (``[CID-03] `rate_limit` is a runtime argument of #[modal] that this Noctis
  release does not support yet``).
- **No modal answers a modal.** `ModalResponder` (`Manual` or not) has no `show_modal()`
  (E0599, INT-04), and any dynamic attempt to show a modal on a submit is
  `ResponderError::ModalUnavailable` with no request.
- Answer a submit with `ctx.update(msg)` (redraw the message the modal was opened
  from) or `ctx.respond(msg)` (a new message); returning the message or the
  `ModalResponder` still work (`responder.md` §1b).

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
  never dropped; a handler reads them as `ctx.unknown_components()` on its
  `ModalSubmitCtx`. Decoding never compares `custom_id` with the Route (dispatch is core's).
- A `signed` modal is verified by component dispatch with the App's payload key; a
  `by_ref` or spilled one (once its Payload overflows) decodes only through component
  dispatch, which reads its record from the App's `RefStore`. `from_submission`/`from_json` alone return
  `SubmitError::Malformed` for them (the Ref or the tag needs the App). A `one_shot`
  by-Ref modal's Ref is taken before its inputs are decoded: a submission rejected as
  `submit_invalid` or `undecodable` consumes it too, and the retry gets the Expired notice.
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
- Submitted values are untrusted (MOD-09): re-validate ranges and authorization. A
  submission that fails to decode (a `SubmitError` such as an unknown radio value, a
  missing required input, too many values or a File Upload id missing from `resolved`)
  never runs the handler: one `warn` on `noctis::dispatch` with `reason="submit_invalid"`
  (`reason="malformed"` for broken JSON) and the generic ephemeral `noctis.error.generic`
  notice (CID-06), never a panic.
- If the modal was opened from a message component, returning a View **updates that
  message** (type 7; the auto-defer is type 6); from a command, there is no message to
  update: a returned View is a new public message (auto-defer type 5), and
  `ModalResponder::update`/`defer_update` and `ctx.update` return
  `ResponderError::UpdateUnavailable` without a request (also after the auto-defer;
  `ctx.update` never falls back to a new message), so answer with `ctx.respond(msg)`
  or `r.reply(msg)`.
  The handler takes `(self, ctx: ModalSubmitCtx[, r: ModalResponder])`, or
  `ModalResponder<Manual>` with `#[modal(.., auto_defer = false)]` (§3.0).
  `ModalResponder` has no `show_modal()`: modals can't chain (INT-04, E0599).
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
(pre-fill) ≤ 4000 and ≤ its own `max_length` (by `discord_len`), `placeholder` ≤ 100
(MOD-05). Lenient mode truncates the placeholder, never the `value`: a too-long value is
an error in both modes.

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
`.default_roles(ids)`, `.default_mentionables([MentionableId::User(id), MentionableId::Role(id)])`
(appends in order; duplicates count once), plus `.placeholder`, `.min_values`, `.max_values`, `.required`.

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
(MOD-07); at most one option with `.default(true)` (MOD-07, both modes). The constructor
accepts any count; `validate` rejects a wrong one. A
`#[radio_group]` field's option count is checked at `cargo check` instead (§1.2).

**Builder:** `RadioGroup::new(key, [ModalOption::new(label, value), …])`, `.required(bool)`.
`ModalOption::new(label, value)`, `.description(..)`, `.default(bool)`; JSON
`{"value","label","description"?,"default":true?}`.

### 4.10 Checkbox Group: `CheckboxGroup`

**Where:** inside a `ModalLabel` only. Type 22.

**Limits:** 1–10 options, `min_values` 0–10 (0 only with `.required(false)`),
`max_values` 1–10, `min ≤ max` (default max = option count); option text as Radio Group;
the count of `.default(true)` options, when there is one, within `[min_values, max_values]`
(MOD-07, both modes).

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

## 4a. Per-render derived modal: `ModalBuilder`

**Where:** what `X { .. }.build()` returns for a `#[derive(Modal)]` struct `X` (and
`X::blank(..)` when an input has no empty value). It is not a component: it converts into
a `Modal` (`.into()`, `Modal::from(b)`) and `show_modal` takes it directly. Never store
it across renders; build a new one each time.

**Limits:** the converted `Modal` is validated with everything else before it is sent, in
every Limit mode: title 1–45 (MOD-01), Label text ≤ 45 and description ≤ 100 (MOD-04), a
Text Input placeholder ≤ 100 and a select's ≤ 150 (Lenient truncates these), a Text Input
`value` ≤ 4000 and ≤ its `max_length` (MOD-05), a Checkbox Group's defaults within
`[min_values, max_values]` and a Radio Group's at most one (MOD-07), a select's defaults
≤ `max_values` (CV2-09). A failure is `ResponderError::Invalid(UiError::Limits { .. })`
and no request is sent. Locale keys are resolved first (User Locale, then Guild Locale,
then en-US); a missing key is sent as the key with one `warn` on `noctis::i18n`.

**Builder:**

```rust
let modal = BioForm { bio: Some("hi".into()), ..BioForm::blank(user) }
    .build()
    .title(MessageKey::new("profile.bio_modal.edit_title")) // a literal or a MessageKey
    .value(BioForm::BIO, Some("replaced".into()))           // typed handle: the field's type
    .overrides(BioFormOverrides {
        bio: InputOverrides { label: Some("About you".into()), ..Default::default() },
        ..Default::default()
    });
Ok(r.show_modal(modal).await?.done())
```

- `.title(t)` replaces the declared title for this render; it is the same field as
  `XOverrides::title`.
- `.value(X::FIELD, v)` replaces one input's pre-fill. The derive generates one handle
  const per input field, its name upper-cased (`bio` → `BioForm::BIO`, of type
  `InputField<BioForm, Option<String>>`); `#[text_display]` and `#[file_upload]` fields
  have none.
- `.overrides(XOverrides { .. })`: the derive generates `XOverrides` with `title:
  Option<Text>` and, per Label input, an `InputOverrides { label, description,
  placeholder }` (each `Option<Text>`). Each `Some` replaces; `InputOverrides` merge field
  by field. A placeholder on a File Upload, Radio Group, Checkbox Group or Checkbox is
  ignored. Overrides change presentation only, never a value, the `custom_id` or the
  decode.
- Calls compose and the later one wins.

## 4b. Building a modal at runtime: `Modal`

`Modal` is the callback `data` of a modal (type 9). It is built only from a modal struct
value (`form.build()` or `Form::blank(..)`), so its `custom_id` is that
struct's encoding (MOD-08) and the submission goes to its handler. There is no
`Modal::new`, no string id and no `Default`; a struct without a `#[handler]` fails with
`[CID-04]`.

```rust
use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::ui::{
    Checkbox, FileUpload, ModalLabel, ModalOption, RadioGroup, TextDisplay, TextInput,
    TextInputStyle, validate,
};

#[derive(Modal)]
#[modal("mod.report", defaults(title = "Report a user"))]
struct Report {
    id: u64,                                                   // Payload
    #[text_display("Tell us what happened.")]
    _intro: (),
}
impl Report {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx) -> HandlerResult<Responded> { … }
}

let mut modal = Report::blank(7)                               // the derived part; push the rest
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
  select nonce or signature is resolved by the Responder before sending;
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
- `r.show_modal(modal)` on a `CommandResponder<Manual>` or a `ComponentResponder<Manual>`
  runs this validator (and fails with `ResponderError::Invalid`) before any request, then
  sends callback type 9.

## 5. Button → modal → re-render (settings screens and step flows)

A setting or a step that needs free text shows a button declared `auto_defer = false`.
The modal struct carries **the same Payload** (the entity id, or the whole step state),
so its handler knows which message to redraw:

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("ticket.text", auto_defer = false, defaults(style = Secondary, label = keys::TICKET_EDIT_TEXT))]
pub struct EditText { pub system: u32 }

#[derive(Modal, Debug)]
#[modal("ticket.text.save", defaults(title = keys::TICKET_TEXT_TITLE))]
pub struct TextForm {
    pub system: u32,                                               // the same Payload
    #[text_input("Button text", max_length = 80)]
    pub text: Option<String>,
}

impl EditText {
    #[handler]
    async fn handle(self, ctx: ComponentCtx, r: ComponentResponder<Manual>, db: State<Db>) -> HandlerResult<Responded> {
        let current = db.button_text(self.system).await?;          // pre-fill from storage
        Ok(r.show_modal(TextForm { system: self.system, text: current }.build()).await?.done())
    }
}

impl TextForm {
    #[handler]
    async fn handle(self, ctx: ModalSubmitCtx, db: State<Db>) -> HandlerResult<V2Message> {
        db.set_button_text(self.system, self.text).await?;        // validate first
        let data = db.ticket_systems(ctx.guild_id()).await?;
        Ok(render_ticket_setup(&data, &Strings::of(ctx.response_locale()), Some(self.system)))
    }
}
```

In a step flow the submit handler returns `render_setup(next_state, &s)` (`response =
update`: it redraws the step's message).
Never answer a modal submit with another modal (INT-04): render a message step first.

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
