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
buttons = 6, Action Row + 1 select (25 options) = 2. Every type exposes
`const COST`; `view.cost()` / the testkit report actual totals.

---

## 1. Container (type 17)

- **Where:** message top level only. **Not** inside another Container (CV2-05).
- **Children:** Action Row, Text Display, Section, Media Gallery, Separator, File.
- **Fields:**
  | Field | Builder | Limit |
  | --- | --- | --- |
  | `components` | `.push(child)`, `.text(..)`, `.section(..)`, `.row(..)`, `.gallery(..)`, `.file(..)`, `.separator(..)` | only the 40 total |
  | `accent_color` | `.accent(Rgb::new(0x5865_F2))` | 0x000000–0xFFFFFF, checked in `const fn` (CV2-13) |
  | `spoiler` | `.spoiler(true)` | — |
- **Cost:** 1.
- `ContainerChild` is an enum with `From` for every allowed child, so `vec![a.into(), b.into()]` works.

```rust
Container::new().accent(Rgb::new(0x57_F287)).text("### Saved").separator(Separator::small())
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
