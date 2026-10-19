# Components V2: every component

Covers each message component one by one: type id, where it may appear, every field
with its limit and rule id, its cost, and the Noctis builder. Modal-only components
are in `modals.md`. All limits live in `Limits` (see `limits-cheatsheet.md`); placement
is enforced by types, so illegal nesting does not compile (CV2-05).

Builder names are [planned] where the spine leaves them open; the shapes (typed
placement, const-generic arity, construction from Routes) are fixed.

## 0. The message

```rust
V2Message::new()                            // always IS_COMPONENTS_V2 (1<<15); can't be removed (CV2-01)
    .container(Container::new() …)          // or .text(..), .section(..), .row(..), .gallery(..), .file(..), .separator(..)
    .mentions(AllowedMentions::none())      // default for Views (MSG-05)
```

| Rule | Limit |
| --- | --- |
| CV2-02 | no `content`, `embeds`, `poll`, `sticker_ids` (the type has no such fields) |
| CV2-03 | ≤ **40** components in total, counting every nesting level. Select options and gallery items don't count |
| CV2-04 | ≤ **4000** characters across all Text Displays (larger of scalars and UTF-16 units) [U] |
| CID-02 / CV2-14 | `custom_id`s and numeric `id`s unique per message (Noctis assigns `id`s deterministically) |
| ATT-03 | ≤ 10 attachments [U] |

Top level of a message: Container, Section, Text Display, Media Gallery, File,
Separator, Action Row. No separate top-level cap (only the 40 total).

In Views you rarely build `V2Message` by hand: `Layout::new()…build(cx)` does it, adds
Chrome and paginates (see `views-and-widgets.md`).

Cost cheatsheet: Container 1, Text 1, Separator 1, Media Gallery 1 (any items),
File 1, Section + 1 text + button = 3, Section + 3 texts + thumbnail = 5, Action Row + 5
buttons = 6, Action Row + 1 select (25 options) = 2. `components!` (§14) counts a
literal tree at compile time; `view.cost()` / the testkit report actual totals.

---

## 1. Container (type 17)

- **Where:** message top level only. **Not** inside another Container (CV2-05).
- **Children:** Action Row, Text Display, Section, Media Gallery, Separator, File.
- **Fields:**
  | Field | Builder | Limit |
  | --- | --- | --- |
  | `components` | `.push(child)`, `.text(..)`, `.section(..)`, `.row(..)`, `.gallery(..)`, `.file(..)`, `.separator(..)` | only the 40 total |
  | `accent_color` | `.accent(rgb!(0x5865_F2))` | 0x000000–0xFFFFFF, checked by the `rgb!` macro at compile time; runtime values: `Rgb::try_from(u32)?` (CV2-13) |
  | `spoiler` | `.spoiler(true)` | — |
- **Cost:** 1.
- `ContainerChild` is an enum with `From` for every allowed child, so `vec![a.into(), b.into()]` works.

```rust
Container::new().accent(rgb!(0x57_F287)).text("### Saved").separator(Separator::small())
```

Use one Container per View, an accent colour per state (neutral / success / danger).

## 2. Text Display (type 10)

- **Where:** top level, Container, Section (1–3), modals (top level).
- **Fields:** `content` (Markdown: `#`/`##`/`###`, `-#` subtext, lists, code, mentions,
  `<t:unix:R>` timestamps). No per-component maximum documented; counts toward the
  4000 total (CV2-04).
- **Builder:** `Text::new(impl Into<String>)`. Literal check: `lit!("…")` [planned].
- **Cost:** 1.
- Mentions inside Text Display **would ping**; Views send `parse: []` by default.
  Escape user text with `noctis::ui::escape_markdown` / `escape_mentions`.
- Merge adjacent static text into one Text Display: saves components, not chars.

## 3. Section (type 9)

- **Where:** top level or Container. Messages only.
- **Fields:**
  | Field | Limit | Rule |
  | --- | --- | --- |
  | `components` | 1–3 Text Displays | CV2-07 |
  | `accessory` | exactly one: Button (any style) or Thumbnail | CV2-07 |
- **Builder:** `Section::new([Text; N], accessory)` — `N` checked by const assert;
  there is no constructor without an accessory. Runtime: `Section::try_new(Vec<Text>, acc)?`.
- **Cost:** 1 + texts + 1.
- Pattern: settings row = value text + "Edit"/"Set" button (Primary when unset,
  Secondary when set); toggle row = state text + Enable (Success) / Disable (Danger).

```rust
Section::new([Text::new(format!("**{name}** · {state}"))], toggle_button)
```

## 4. Thumbnail (type 11)

- **Where:** **only** as a Section accessory (CV2-05).
- **Fields:**
  | Field | Builder | Limit |
  | --- | --- | --- |
  | `media.url` | `Thumbnail::new(media)`; `media` = `cx.attach(&att)` or `Media::url(url)` | https or `attachment://` (ATT-05); URL ≤ 2048 [U] |
  | `description` | `.alt(text)` | ≤ 1024 (CV2-11); missing alt = warning (strict: error for rendered images, FR-30) |
  | `spoiler` | `.spoiler(true)` | — |
- Images only (GIF/WebP animate), **no video**. Small on mobile: never put
  information-bearing text in it. **Cost:** 1.

## 5. Media Gallery (type 12)

- **Where:** top level or Container.
- **Fields:**
  | Field | Limit | Rule |
  | --- | --- | --- |
  | `items` | 1–10 | CV2-10 |
  | item `media.url` | https or `attachment://` | ATT-05 |
  | item `description` (alt) | ≤ 1024 | CV2-10 |
  | item `spoiler` | bool | — |
- **Builder:** `MediaGallery::new([MediaItem; N])` (const assert 1..=10) or
  `MediaGallery::try_from_vec(vec)?`. `MediaItem::new(media).alt(..).spoiler(..)`.
- **Cost:** 1 regardless of items — the cheapest way to show visual data. 1 item = full
  width; 2–4 = grid. Images and video.

```rust
MediaGallery::new([MediaItem::new(cx.attach(&self.card)).alt(s.profile.card_alt(&name))])
```

## 6. File (type 13)

- **Where:** top level or Container.
- **Fields:** `file` = **only** `attachment://<name>` (CV2-12); `spoiler`. `name`/`size`
  are response-only.
- **Builder:** `File::new(cx.attach(&att))` — an attachment reference can only come from
  attaching, so dangling names are impossible. One file per File component. **Cost:** 1.

## 7. Separator (type 14)

- **Where:** top level or Container.
- **Fields:** `divider` (default true), `spacing` 1 small / 2 large (CV2-13).
- **Builder:** `Separator::small()`, `Separator::large()`, `.divider(false)`. **Cost:** 1.
- Large before major sections, small inside.

## 8. Action Row (type 1)

- **Where:** top level or Container (in modals: deprecated; Noctis never emits it).
- **Content:** 1–5 Buttons **or** exactly 1 select, never mixed (CV2-06).
- **Builder:** `ActionRow::buttons([Button; N])` (const assert 1..=5),
  `ActionRow::select(select)`, runtime `ActionRow::try_buttons(Vec<Button>)?`.
- **Cost:** 1 + children. Rows wrap on mobile beyond ~3 labelled buttons; keep labels short.

## 9. Button (type 2)

| Field | Limit | Rule |
| --- | --- | --- |
| `style` | 1 Primary, 2 Secondary, 3 Success, 4 Danger, 5 Link, 6 Premium | CV2-08 |
| `label` | ≤ 80 (guideline: ≤ 34 with emoji, 38 without) | CV2-08 |
| `emoji` | `Emoji::unicode("🔄")`, `Emoji::custom(id, "name", animated)` | — |
| `custom_id` | styles 1–4 only; built from the Route, never by hand | CID-01..07 |
| `url` | Link only, ≤ 512 | CV2-08 |
| `sku_id` | Premium only | CV2-08 |
| `disabled` | bool | — |

Builders:
- Route button: `MyRoute { .. }.button()` → `Button`; `.label(..)`, `.style(ButtonStyle::Primary)`
  (default Secondary), `.emoji(..)`, `.disabled(bool)`. Sends an interaction to the Route's handler.
- Link: `Button::link(url).label(..)` — no interaction, no `custom_id`.
- Premium: `PremiumButton::new(sku_id)` — no label/emoji/url (the type has no setters); `Into<Button>`.
- View controls: `cx.reload_button()` (re-runs the View's loader with the current
  tab/page/step); Navbar/Wizard/Paginator buttons are generated.

Where: Action Row or Section accessory. Never in modals. Guidelines: one Primary per
group; equal-weight actions all Secondary; Danger only for destructive actions.

## 10. Select menus (types 3, 5, 6, 7, 8) — message variants

Route must declare `select` (adds the per-render nonce).

| Type | Builder | Values in handler |
| --- | --- | --- |
| 3 String Select | `route.string_select([SelectOption; N])` or `try_string_select(vec)?` | `ctx.values().strings()` |
| 5 User Select | `route.user_select()` | `ctx.values().users()` + `ctx.resolved()` |
| 6 Role Select | `route.role_select()` | `.roles()` |
| 7 Mentionable Select | `route.mentionable_select()` | `.mentionables()` |
| 8 Channel Select | `route.channel_select().channel_types([ChannelType::GuildText])` | `.channels()` |

Shared fields:

| Field | Builder | Limit | Rule |
| --- | --- | --- | --- |
| `placeholder` | `.placeholder(..)` | ≤ 150 | CV2-09 |
| `min_values` | `.min_values(n)` | 0–25 (default 1) | CV2-09 |
| `max_values` | `.max_values(n)` | 1–25 (default 1), ≥ min | CV2-09 |
| `disabled` | `.disabled(bool)` | messages only | CV2-09 |
| `default_values` (5–8) | `.default_users(..)`, `.default_roles(..)`, `.default_channels(..)` (any `IntoIterator<Item = Id<_>>`, `Option` works) | count within [min, max] | CV2-09 |
| `options` (3) | `SelectOption::new(label, value).description(..).emoji(..).default(true)` | ≤ 25 options; label/value/description ≤ 100 | CV2-09 |

- **Cost:** 2 (row + select). Options don't count.
- After every re-render, selects show the View's defaults (nonce, FR-25); always set
  `default`/`default_*` to the current value (visible active state).
- Empty list: render a disabled select with one explanatory option, or an empty-state
  Text, never nothing.
- More than 25 items: `Paginator::…as_select(..)` chunks into several selects.
- `derive(Choices)` enums can produce options: `Severity::select_options(locale)` [planned].

## 11. Raw escape hatch (FR-31)

`RawComponent::new(serde_json::Value)` places JSON for a component type Noctis doesn't
model; it still counts toward CV2-03. Received unknown components are available as raw
JSON (`ctx.raw_message_components()`) and never re-sent on edit.

## 12. Legacy messages

`LegacyMessage` (content ≤ 2000, ≤ 5 Action Rows, types 1–8; CV2-15). Not for
interactive UI (UD-1).

## 13. Where each component may appear

| Component | Msg top | Container | Section | Action Row | Modal |
| --- | --- | --- | --- | --- | --- |
| Container 17 | yes | no | — | — | no |
| Section 9 | yes | yes | — | — | no |
| Text Display 10 | yes | yes | 1–3 | — | top level |
| Thumbnail 11 | no | no | accessory only | — | no |
| Media Gallery 12 | yes | yes | — | — | no |
| File 13 | yes | yes | — | — | no |
| Separator 14 | yes | yes | — | — | no |
| Action Row 1 | yes | yes | — | — | deprecated |
| Button 2 | no | no | accessory | 1–5 | no |
| Selects 3,5–8 | no | no | — | alone | inside Label |
| Label 18, Text Input 4, File Upload 19, Radio 21, Checkbox Group 22, Checkbox 23 | — | — | — | — | see `modals.md` |

## 14. `components!`: literal trees checked at compile time [planned]

For a fixed-shape message, write the tree with `components!` (FR-78). It expands to the
builders above and fails `cargo check` with the rule id when the literal tree breaks a
rule: placement (CV2-05), more than 40 components (CV2-03), more than 4000 literal text
characters (CV2-04), row/section/gallery arity (CV2-06, CV2-07, CV2-10), over-long
literal labels, placeholders or alt text (CV2-08, CV2-09, CV2-11), and two identical
literal Route components (CID-02, same Route and same literal Payload).

```rust
let body = components! {
    container(accent = rgb!(0x5865_F2)) [
        text("### Server settings"),
        separator(spacing = small),
        section [ text("Logs go to the channel you pick."); accessory = SetLogs { guild }.button(label!("Change")) ],
        row [ Save { guild }.button(label!("Save")), Cancel { guild }.button(label!("Cancel")) ],
        ..extra_rows,                 // spread: an iterator of nodes, checked at runtime
    ],
    {footer},                         // hole: one node you built elsewhere
};
V2Message::new().components(body)
```

Node keywords: `container`, `section` (texts `;` `accessory = …`), `text`, `thumbnail`,
`gallery [ item(..), … ]`, `file`, `separator`, `row`; buttons and selects are Route
values with their constructor (`.button(..)`, `.string_select(..)`, …), plus
`link(url = "…", label = "…")` and `premium(sku)`. A hole counts as 1 component, a
spread as 0; their real size and all non-literal text are checked by the runtime
validator before sending. `view!` is the unrelated image Template macro.

## 15. Rule functions (`noctis::rules::component`, `message`, `attachment`)

The `noctis-ui` builders already make most shapes unrepresentable. These functions check
trees and values Noctis cannot see at compile time (dynamic trees, raw components,
user-supplied labels, colours, attachments) with the same counts, paths and order that
`components!` and the runtime validator use. They only report: they never truncate (in
`LimitMode::Lenient` the Responder truncates display text, never structure). Every
function takes `&Limits`; never hard-code a limit.

```rust
use noctis::rules::Limits;
use noctis::rules::component::{
    Accessory, Button, ButtonStyle, Component, Container, Section, cost, validate_components,
};

let limits = Limits::default();
let texts = ["Logs go to the channel you pick."];
let row = [Component::Button(Button::new(ButtonStyle::Primary).label("Save"))];
let children = [
    Component::Text("### Server settings"),
    Component::Section(Section::new(&texts, Accessory::Button(Button::new(ButtonStyle::Secondary).label("Change")))),
    Component::Row(&row),
    Component::Opaque(1), // a node built elsewhere (raw JSON or a hole)
];
let tree = [Component::Container(Container::new(&children).accent_color(0x5865F2))];
validate_components(&tree, &limits).map_err(report)?;   // Err(Vec<Violation>), never empty
assert_eq!(cost(&tree, &limits).components, 8);
```

### Descriptor types

All are `Clone + Copy + Debug + PartialEq` and borrow their data; structs are
`#[non_exhaustive]` with pub fields, a `const fn new` and by-value `const` setters named
after the fields.

| Item | What it is |
| --- | --- |
| `Component<'a>` | `Container(Container)`, `Section(Section)`, `Text(&str)`, `Thumbnail(Media)` (valid only as an accessory), `Gallery(&[Media])`, `File(&str)` (`attachment://` URL), `Separator(Separator)`, `Row(&[Component])`, `Button(Button)`, `Select(Select)`, `Opaque(usize)`; `.kind()` → the path name (`container`, `section`, `text`, `thumbnail`, `gallery`, `file`, `separator`, `row`, `button`, `select`, `opaque`) |
| `Opaque(n)` | `n` raw/unknown components (CV2-16) or a `components!` hole: counts `max(n, 1)` toward CV2-03, allowed anywhere, one Action Row item |
| `Container<'a>` | `{ children, accent_color: Option<u32>, spoiler }`; `Container::new(&children)`, `.accent_color(0x5865F2)`, `.spoiler(bool)` |
| `Section<'a>` | `{ texts: &[&str], accessory: Accessory }`; `Section::new(&texts, accessory)` (no constructor without an accessory, CV2-07) |
| `Accessory<'a>` | `Button(Button)` or `Thumbnail(Media)` |
| `Media<'a>` | gallery item or thumbnail: `{ url, description: Option<&str>, spoiler }`; `Media::new(url)`, `.description(..)`, `.spoiler(..)` |
| `Separator` | `{ divider: bool, spacing: Spacing }`; `Separator::new()` / `Default` = divider, small; `.divider(..)`, `.spacing(..)` |
| `Spacing` | `Small` (default, `.value()` 1) or `Large` (2); no other value exists (CV2-13) |
| `ButtonStyle<'a>` | `Primary`, `Secondary`, `Success`, `Danger`, `Link(url)`, `Premium`; `.value()` → 1..=6 |
| `Button<'a>` | `{ style, label: Option<&str>, emoji: bool }`; `Button::new(style)`, `.label(..)`, `.emoji(bool)`; no `custom_id` (Routes own it) |
| `SelectKind<'a>` | `String(&[SelectOption])`, `User`, `Role`, `Mentionable`, `Channel` |
| `Select<'a>` | `{ kind, placeholder, min_values: Option<usize>, max_values: Option<usize>, default_values: usize, disabled }`; `Select::new(kind)`, `.placeholder(..)`, `.min_values(n)`, `.max_values(n)`, `.default_values(count)` (auto-populated selects only), `.disabled(bool)` |
| `SelectOption<'a>` | `{ label, value, description, default }`; `SelectOption::new(label, value)`, `.description(..)`, `.default(bool)` |
| `Cost` | `{ components, text_len }` (`#[non_exhaustive]`, `Copy`, `PartialEq`) |

### Functions

| Function | Rules |
| --- | --- |
| `validate_components(&[Component], &Limits) -> Result<(), Vec<Violation>>` | a V2 message body: CV2-03..13, CV2-16, CV2-17 |
| `validate_legacy_components(&[Component], &Limits) -> Result<(), Vec<Violation>>` | a legacy message: CV2-15 (≤ `legacy_action_rows` top-level rows, nothing else; `Opaque` counts as a row), then each row as above; never CV2-03/CV2-04 |
| `cost(&[Component], &Limits) -> Cost` | CV2-03 count (every node at every depth, Section texts and accessory included, `Opaque(n)` = `max(n, 1)`, options and gallery items not counted) and CV2-04 `discord_len` sum of every Text Display; saturating |
| `validate_button_label(&str, &Limits) -> Vec<Violation>` | CV2-08 label ≤ 80 (empty is fine); path `""` (what `label!` calls) |
| `validate_link_url(&str, &Limits)` | CV2-08 link URL 1–512 |
| `validate_select_placeholder(&str, &Limits)` | CV2-09 placeholder ≤ 150 |
| `validate_gallery_description(&str, &Limits)` / `validate_thumbnail_description(&str, &Limits)` | CV2-10 / CV2-11 alt text ≤ 1024 |
| `validate_media_url(&str, &Limits)` | CV2-17 media URL 1–2048 |
| `validate_accent_color(u32, &Limits)` | CV2-13 `0x000000..=0xFFFFFF` (what `rgb!` calls); actual printed as `` `0x1000000` `` |
| `validate_spacing(u8, &Limits)` | CV2-13 raw spacing 1 or 2 |

Paths: top-level node `i` is `kind[i]`, a child `parent.kind[j]` (`j` counts all
siblings), a Section accessory `P.accessory`, a gallery item `P.item[k]`, a select option
`P.options[k]`; fields append `.label`, `.url`, `.emoji`, `.placeholder`, `.min_values`,
`.max_values`, `.options`, `.default_values`, `.description`, `.accent_color`. Child
counts use the node path; CV2-03 and CV2-04 use `components`. Example:
`container[0].row[2].button[1].label`.

Order: CV2-03 (`AtMost(40)`, one note per top-level node such as `text[0]: 1`), CV2-04,
then a pre-order walk; per node its CV2-05 placement check, then its own checks:
Container CV2-13 then children; Section CV2-07 (1–3 texts) then the accessory; Gallery
CV2-10 count (1–10), then per item URL (CV2-17) and description (CV2-10); File CV2-12
(`attachment://<filename>`); Row CV2-06 (1–5 items, or one select alone: `exactly one
select, alone in its row`) then items; Button label (≤ 80, or `no label on a premium
button`), link URL, premium emoji; Select placeholder, `min_values` (0–25), `max_values`
(1–25), `min_values ≤ max_values` (effective defaults min 1, max 1; only when both are in
range), String options (1–25; label and value 1–100, description ≤ 100), then the default
count (`default_values`, or `default`-flagged String options) within `[min, max]` when
there is at least one and the order passed (`default_values` on a String Select is
CV2-09); a misplaced Thumbnail is still checked (URL, description). `disabled` is fine in
messages. Placement texts: `a Thumbnail only as a Section accessory`, `a Button only in an
Action Row or as a Section accessory`, `a select only in an Action Row`, `no Container
inside a Container`, `only buttons and selects in an Action Row`.

### Messages, webhooks (`noctis::rules::message`)

| Item | What it is |
| --- | --- |
| `validate_content(&str, &Limits) -> Vec<Violation>` | MSG-01 legacy `content` ≤ 2000, path `content` |
| `MentionIds<'a>` | `None` (default), `All` (`parse`) or `Only(&[u64])`; `parse` plus ids cannot be built (MSG-03) |
| `AllowedMentions<'a>` | `#[non_exhaustive]` `{ users, roles: MentionIds, everyone, replied_user }`; `AllowedMentions::new()` = `Default` = pings nobody (MSG-05); `.users(..)`, `.roles(..)`, `.everyone(bool)`, `.replied_user(bool)` |
| `validate_mentions(&AllowedMentions, &Limits)` | MSG-04 ≤ 100 ids each, paths `allowed_mentions.users`, `allowed_mentions.roles` |
| `validate_nonce(&str, &Limits)` / `validate_sticker_ids(count, &Limits)` | MSG-06 nonce ≤ 25 (path `nonce`), ≤ 3 stickers (path `sticker_ids`) |
| `validate_webhook_name(&str, &Limits)` | WH-04 1–80, path `name` |

Noctis has no embed API in v1 (MSG-02), so there is nothing to check for embeds.

### Attachments (`noctis::rules::attachment`)

| Item | What it is |
| --- | --- |
| `AttachmentDecl<'a>` | `#[non_exhaustive]` `{ filename, size: usize, description: Option<&str> }`; `AttachmentDecl::new("card.png", bytes.len())`, `.description(alt)` |
| `validate_attachments(&[AttachmentDecl], json_len, file_limit: Option<usize>, &Limits) -> Result<(), Vec<Violation>>` | ATT-03 count (≤ 10, path `attachments`), per file ATT-01 size (`attachments[k].size`) and ATT-06 alt text ≤ 1024 (`attachments[k].description`), each with note ``file `name` ``, then ATT-02 request total (files + `json_len`, saturating, ≤ 25 MiB, path `request`) |
| `file_limit` | the interaction's `attachment_size_limit`; when `Some`, it replaces `Limits::attachment_bytes` (20 MiB) |
| `validate_file_size(size, file_limit, &Limits)` / `validate_attachment_description(&str, &Limits)` | single ATT-01 / ATT-06 checks, path `""` |
| `validate_attachment_ref(url, &[AttachmentDecl], &Limits) -> Vec<Violation>` | ATT-05: an `attachment://<name>` URL must name one of the files (exact, case-sensitive); other URLs pass |
