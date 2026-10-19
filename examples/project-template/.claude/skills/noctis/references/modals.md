# Modals

Covers: `#[derive(Modal)]`, Labels, every modal component (Text Input, selects, File
Upload, Radio Group, Checkbox Group, Checkbox, Text Display), pre-fill, opening a modal,
typed submissions, modal-based Wizard Steps. Rules: MOD-01..09, INT-05, CV2-09, FR-29,
FR-63, FR-64.

## 1. Anatomy

A modal is a form struct plus the Route its submission goes to:

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

### Label (type 18)
| Field | Attribute | Limit |
| --- | --- | --- |
| `label` | `#[label("…")]` / `#[label(key = "…")]` | ≤ 45 (MOD-04) |
| `description` | `description = "…"` / `description(key = "…")` | ≤ 100 (MOD-04); renders above or below by platform |
| `component` | the field's input attribute | exactly one |

Cost: Label + child = 2 components.

### Text Input (type 4)
`#[text_input(style = short | paragraph, min_length = n, max_length = n, placeholder = "…")]`
on `String` / `Option<String>` (or a `FromStr` type [planned]).

| Field | Limit |
| --- | --- |
| `min_length` | 0–4000 (MOD-05) |
| `max_length` | 1–4000 |
| `value` (pre-fill) | ≤ 4000 |
| `placeholder` | ≤ 100 |
| `required` | from `Option` |

### Select menus inside a Label (types 3, 5–8)
| Attribute | Field type |
| --- | --- |
| `#[string_select(min_values = 1, max_values = 3)]` | `Vec<E>` / `E` / `Option<E>` with `E: Choices` |
| `#[user_select]` | `ResolvedUser`, `Vec<ResolvedUser>` |
| `#[role_select]` | `ResolvedRole`, `Vec<…>` |
| `#[mentionable_select]` | `Mentionable`, `Vec<…>` |
| `#[channel_select(channel_types(GuildText))]` | `ResolvedChannel`, `Vec<…>` |

Limits as in messages (CV2-09: placeholder ≤ 150, min 0–25, max 1–25, ≤ 25 options)
except: `required` exists (default true) and `disabled` does not. `min_values = 0`
with `required = true` is rejected.

### File Upload (type 19)
`#[file_upload(min_values = 0, max_values = 5, file_types = ["image", ".pdf"])]` on
`Vec<ResolvedAttachment>` / `Option<ResolvedAttachment>`.

| Field | Limit |
| --- | --- |
| `min_values` | 0–10 (MOD-06) |
| `max_values` | 1–10 |
| `file_types` | ≤ 10 entries; `image`, `video`, `audio` or `.ext`; extension-only check |

Size is capped by the user's upload limit. URLs are **ephemeral**: download promptly
with `attachment.download(&ctx).await?` [planned] and re-check the content type.

### Radio Group (type 21)
`#[radio_group]` on `E` / `Option<E>` with `E: Choices`. 2–10 options (MOD-07; checked
from the enum's variants). Submit value: one option or none.

### Checkbox Group (type 22)
`#[checkbox_group(min_values = 0, max_values = 3)]` on `Vec<E>`. 1–10 options,
`min_values` 0–10, `max_values` 1–10 (MOD-07).

### Checkbox (type 23)
`#[checkbox]` on `bool`. Can't be `required` (MOD-07); for "must accept", use a
1-option Checkbox Group with `min_values = 1`.

### Text Display (type 10)
`#[text(key = "…")]` on a unit field `intro: ()` [planned]: static explanatory text at
the top level; counts toward the 5.

## 5. Modals in Wizards

A Step that needs free text shows a button (an `opens_modal` Route carrying the
Wizard state). The modal's submit Route also carries the state; its handler returns
`MyWizard::new(new_state).at(Nav::step(k + 1))`, updating the Wizard message.

## 6. Choosing modal vs message components

- Message selects fire on every change and are visible to everyone on a public message.
- Modals batch several inputs into one atomic submit and collect free text.
- Use a modal for 2+ related inputs, free text, numbers, files; message selects for
  single picks with live feedback.
