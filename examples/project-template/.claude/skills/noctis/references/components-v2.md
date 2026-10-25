# Components V2: every component

Covers each message component one by one: type id, where it may appear, every field
with its limit and rule id, its cost, and the Noctis builder. Modal-only components
are in `modals.md`. All limits live in `Limits` (see `limits-cheatsheet.md`); placement
is enforced by types, so illegal nesting does not compile (CV2-05).

Everything here is in `noctis::ui` (`use noctis::ui::*;` or name the items); the kind
derives that give `.build()` are in the prelude. Every builder takes text as `impl Into<Cow<'static,
str>>` (a `&'static str` or a `String`), setters are by value and chainable, and every
type is `Debug + Clone + Send + Sync` and serializes straight to Discord JSON with unset
fields and `false` flags left out.

## 0. The message body: `Components`

```rust
use noctis::prelude::*;
use noctis::ui::{Components, Container, Separator, TextDisplay};

let body = Components::new()                 // a V2 message's top-level components
    .push(Container::new().push(TextDisplay::new("### Saved")))
    .push(Separator::small());
assert_eq!(body.len(), 2);                   // also is_empty(), Default
```

- `Components::push(c)` takes any top-level component (`TopLevel` bound, CV2-05).
- Numeric ids (CV2-14): `Components` numbers every component in pre-order from 1
  (Container 1, its first child 2, …, accessories included, select options and gallery
  items not). No API sets an `id`, so ids are deterministic and unique; a component
  serialized on its own has none.
- `V2Message::new(body)` (or `V2Message::from(body)`) wraps it into a message whose
  every body sets `IS_COMPONENTS_V2` (CV2-01) and has no `content`/`embeds` (CV2-02);
  see §12.

| Rule | Limit |
| --- | --- |
| CV2-02 | no `content`, `embeds`, `poll`, `sticker_ids`, `shared_client_theme` (`V2Message` has no such fields) |
| CV2-03 | ≤ **40** components in total, counting every nesting level. Select options and gallery items don't count |
| CV2-04 | ≤ **4000** characters across all Text Displays (larger of scalars and UTF-16 units) [U] |
| CID-02 / CV2-14 | `custom_id`s and numeric `id`s unique per message (Noctis assigns `id`s deterministically) |
| ATT-03 | ≤ 10 attachments [U] |

Counts, lengths and the other per-field Limits are checked at runtime by
`noctis::ui::validate(&mut msg, &limits)` (§16; the Responder runs it before every
send) and at `cargo check` by `components!` for literal trees (§14);
`noctis::ui::cost(&msg, &limits)` measures a message, and the rule functions of §15
check bare descriptor trees.

A View is a render function that builds this message itself (`views-and-recipes.md`).
Noctis never adds pagination or extra rows to it: size lists with
`noctis::ui::Budget` (`Budget::of(&fixed, ctx.limits()).fits(&row)`, §16 and
`views-and-recipes.md` §4).

Cost cheatsheet: Container 1, Text Display 1, Separator 1, Media Gallery 1 (any items),
File 1, Section + 1 text + button = 3, Section + 3 texts + thumbnail = 5, Action Row + 5
buttons = 6, Action Row + 1 select (25 options) = 2.

## Where each component can be used (placement matrix)

✅ allowed · ❌ rejected at `cargo check` with the rule id · ➖ not applicable.
Message columns are Components V2 messages; the last column is a legacy (non-V2)
message. Modal columns are a `Modal`'s top level and the single input inside a
`ModalLabel`. The foreign webhook column is a `StaticMessage`, the only message a webhook
the App does not own can send (WH-02, §12b).

| Component (type) | Noctis type | Message top level | Container child | Section text | Section accessory | Action Row | Modal top level | Inside a modal Label | Legacy message | Foreign webhook (`StaticMessage`) |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Action Row (1) | `ActionRow` | ✅ | ✅ | ❌ | ❌ | ➖ | ❌ (deprecated in modals; Noctis never emits it) | ❌ | ✅ up to 5 rows | ✅ `StaticActionRow`, Link buttons only |
| Button (2) | `RouteButton<R>`, `LinkButton`, `PremiumButton` | ❌ | ❌ | ❌ | ✅ | ✅ 1-5 per row | ❌ | ❌ | ✅ in a row | ✅ `LinkButton` only (row or accessory); ❌ `RouteButton`, `PremiumButton`, `Button` (WH-02) |
| String Select (3) | `StringSelect` / `ModalStringSelect` | ❌ | ❌ | ❌ | ❌ | ✅ alone in its row | ❌ | ✅ (`ModalStringSelect`) | ✅ alone in a row | ❌ (WH-02) |
| Text Input (4) | `TextInput` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |
| User / Role / Mentionable / Channel Select (5-8) | `UserSelect`… / `ModalUserSelect`… | ❌ | ❌ | ❌ | ❌ | ✅ alone in its row | ❌ | ✅ (`Modal*Select`) | ✅ alone in a row | ❌ (WH-02) |
| Section (9) | `Section` | ✅ | ✅ | ➖ | ➖ | ❌ | ❌ | ❌ | ❌ | ✅ `StaticSection` (Thumbnail or `LinkButton` accessory) |
| Text Display (10) | `TextDisplay` | ✅ | ✅ | ✅ 1-3 per Section | ❌ | ❌ | ✅ | ❌ | ❌ | ✅ |
| Thumbnail (11) | `Thumbnail` | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ as a `StaticSection` accessory |
| Media Gallery (12) | `MediaGallery` | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| File (13) | `File` | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| Separator (14) | `Separator` | ✅ | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ |
| Container (17) | `Container` | ✅ | ❌ (no nesting) | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ `StaticContainer` |
| Label (18) | `ModalLabel` | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ (wraps exactly one input) | ➖ | ❌ | ❌ |
| File Upload (19) | `FileUpload` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |
| Radio Group (21) | `RadioGroup` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |
| Checkbox Group (22) | `CheckboxGroup` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |
| Checkbox (23) | `Checkbox` | ❌ | ❌ | ❌ | ❌ | ❌ | ❌ | ✅ | ❌ | ❌ |

How it's enforced, and what to remember:

- **Rule ids.** Messages: CV2-05 (placement), CV2-06 (a row holds buttons or one
  select), CV2-07 (Section), CV2-15 (legacy). Modals: MOD-02 (1-5 top-level
  components), MOD-03 (allowed children, no `disabled`). The traits are sealed:
  `TopLevel`, `ContainerChild`, `SectionAccessory`, `RowItem`, `RowSelect`,
  `ModalTopLevel`, `ModalInput`. A wrong placement is a compile error naming the rule
  id, never a runtime 400.
- **Selects in modals are separate types.** `ModalStringSelect`,
  `ModalUserSelect`, `ModalRoleSelect`, `ModalMentionableSelect` and
  `ModalChannelSelect` go inside a `ModalLabel`. They have `required` and no
  `disabled`. The message selects (`StringSelect`, …) don't implement `ModalInput`.
- **Modal-only inputs.** Text Input, File Upload, Radio Group, Checkbox Group and
  Checkbox exist only in modals. To collect free text in a message flow, open a modal
  (see `modals.md` §6, "Choosing modal vs message components").
- **No modal buttons.** Buttons, Sections, Containers, galleries and files never go in
  a modal. A modal's only action is its Submit, which Discord renders itself.

### Placement traits

Each placing method is bounded by a sealed trait; only Noctis types implement them, and
a wrong component fails `cargo check` with the rule id:

| Trait | Bound on | Implemented by | Error |
| --- | --- | --- | --- |
| `TopLevel` | `Components::push` | `Container`, `Section`, `TextDisplay`, `MediaGallery`, `File`, `Separator`, `ActionRow`, `TopLevelComponent` | ``[CV2-05] `X` cannot be a top-level message component`` |
| `ContainerChild` | `Container::push` | `Section`, `TextDisplay`, `MediaGallery`, `File`, `Separator`, `ActionRow`, `ContainerComponent` | ``[CV2-05] `X` cannot be a Container child`` |
| `SectionAccessory` | `Section::new` | `RouteButton<R>`, `LinkButton`, `PremiumButton`, `Button`, `Thumbnail` | ``[CV2-07] `X` is not a Section accessory: …`` |
| `RowItem` | `ActionRow::buttons` | `RouteButton<R>`, `LinkButton`, `PremiumButton`, `Button` | ``[CV2-05] `X` cannot be an Action Row button`` |
| `RowSelect` | `ActionRow::select` | the five selects | ``[CV2-06] `X` is not a message select`` |

For lists built at runtime, `ContainerComponent` holds any Container child and
`TopLevelComponent` any top-level component; both convert with `From`/`.into()`:

```rust
let items: Vec<ContainerComponent> = vec![TextDisplay::new("a").into(), Separator::new().into()];
let container = items.into_iter().fold(Container::new(), Container::push);
```

### Components from Routes: `.build()`

Buttons and selects that send an interaction are built only from a Route value (AD-19);
there is no string `custom_id` constructor. The struct's derive names its kind
(`#[derive(Button)]`, `#[derive(StringSelectMenu)]`, `#[derive(UserSelectMenu)]`,
`#[derive(RoleSelectMenu)]`, `#[derive(MentionableSelectMenu)]`,
`#[derive(ChannelSelectMenu)]`) with mandatory `defaults(..)`, and `X { .. }.build()`
returns that component with the defaults applied; the setters below are per-render
overrides (`routes-payloads-refs.md` §1 and §9). The withdrawn per-kind constructor methods are compile errors whose help
says ``write `Route { .. }.build()` ``.

- The struct needs a `#[handler]` method (`impl X { #[handler] async fn handle(self,
  ctx: ComponentCtx) -> HandlerResult<Responded> }`); without one the derive fails with
  E0080 ``[CID-04] `X` has no #[handler] method, so no component can be built for it``.
  Never implement the hidden
  `Handled`/`ComponentRoute` traits by hand.
- Every select kind carries the per-render nonce (FR-25; it implements
  `noctis::codec::SelectRoute`); there is no `select` argument (`#[string_select("id",
  select)]` is CID-03 with help ``every select kind carries the nonce``).
- A plain Route (no `select` or `signed`) is encoded when you build the
  component. Ids that need a select nonce or a signature are finalized by the
  Responder when the message is sent (one fresh nonce per send, never equal to a nonce of
  the interaction being answered, nor one of the handler run's last eight sends; the App's
  payload key signs; every `by_ref`/spilled Route gets a Ref stored in the App's
  `RefStore`, a new one per send). You never handle them
  (`UiError::SlotMismatch` is the Responder's internal error for a mismatched Ref slot).

## 1. Container: `Container`

- **Where:** message top level only. **Not** inside another Container (CV2-05).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) children count toward the 40 total (CV2-03); `accent_color`
  0x000000–0xFFFFFF (CV2-13, guaranteed by `Rgb`). Cost 1.
- **Builder:** `Container::new()`, `.push(child)` (any `ContainerChild`: Section, Text
  Display, Media Gallery, File, Separator, Action Row), `.accent(Rgb::new(r, g, b))`,
  `.spoiler(bool)`. JSON type 17.
- `Rgb::new(0x58, 0x65, 0xF2)` is `const`; `.value()` gives `0x5865F2`. `rgb!(0x5865_F2)`
  (in the prelude) checks a literal at `cargo check` and works in a `const`
  (`const ACCENT: Rgb = rgb!(0x5865_F2);`); a value above `0xFFFFFF` fails with
  ``[CV2-13] limit a colour from 0x000000 to 0xFFFFFF, actual `0x1000000` ``.
  `Rgb::try_from(u32)` is the runtime path (`Err(UiError::Limits { .. })`, same rule).

```rust
Container::new()
    .accent(Rgb::new(0x57, 0xF2, 0x87))
    .push(TextDisplay::new("### Saved"))
    .push(Separator::small())
```

Use one Container per View, an accent colour per state (neutral / success / danger).

## 2. Text Display: `TextDisplay`

- **Where:** top level, Container, Section (1–3 texts), modals (top level).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) no per-component maximum documented; counts toward the 4000 total
  (CV2-04) and the 40 components. Empty text builds (the validator checks it). Cost 1.
- **Builder:** `TextDisplay::new(content)`; `.content()` reads it back. JSON type 10.
  Content is Markdown: `#`/`##`/`###`, `-#` subtext, lists, code, mentions,
  `<t:unix:R>` timestamps. Literal texts are measured by `components!` (§14).
- Mentions inside Text Display **would ping**; Views send `parse: []` by default.
  Escape user text yourself (nothing is escaped automatically):
  `noctis::ui::escape_markdown(&s)` backslash-escapes `\ * _ ~` `` ` `` `| [ ]`
  everywhere and `# > - +` (and a numbered list's `.`) at a line start, so the text
  renders as typed; `escape_mentions(&s)` puts a zero-width space (U+200B) after `@`
  before `everyone`/`here` and after `<` before `@ # /`, so no mention renders. Both
  are `fn(&str) -> String`, only insert characters, commute (use either order) and are
  not idempotent: escape once. Escaped text counts toward CV2-04 at its escaped length.
- Merge adjacent static text into one Text Display: saves components, not chars.

## 3. Section: `Section`

- **Where:** top level or Container. Messages only.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) 1–3 Text Displays and exactly one accessory, a button (any kind) or a
  Thumbnail (CV2-07). Cost 1 + texts + 1.
- **Builder:** `Section::new([TextDisplay; N], accessory)`. There is no constructor
  without an accessory: passing anything else (even `()`) fails with
  ``[CV2-07] `()` is not a Section accessory: a Section needs exactly one accessory, a
  Button or a Thumbnail``. `N` is bounded by `Arity<N>: SectionTexts` (1..=3): 0 or 4
  texts fail `cargo check` with `[CV2-07] a Section holds 1 to 3 Text Displays`. For a
  count known only at runtime use `Section::try_new(texts, accessory, &limits)` (any
  `IntoIterator<Item = TextDisplay>`; `Err(UiError::Limits { .. })` with CV2-07 and an
  empty path). JSON type 9.
- Pattern: settings row = value text + "Edit"/"Set" button (Primary when unset,
  Secondary when set); toggle row = state text + Enable (Success) / Disable (Danger).

```rust
Section::new([TextDisplay::new(format!("**{name}** · {state}"))], toggle_button)
```

## 4. Thumbnail: `Thumbnail`

- **Where:** **only** as a Section accessory (CV2-05); anywhere else fails with
  ``[CV2-05] `Thumbnail` cannot be a Container child`` (or top-level component).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) `media.url` https or `attachment://` (ATT-05), ≤ 2048 [U] (CV2-17);
  `description` ≤ 1024 (CV2-11); missing alt = warning (Strict: `[ATT-06]` error for
  rendered images, FR-30). Images only (GIF/WebP animate), **no video**. Cost 1.
- **Builder:** `Thumbnail::new(media)` where `media` is a `Media` or an
  `AttachmentRef`; `.alt(text)` (`description`), `.spoiler(bool)`. JSON type 11.
- `Media::url(url)` is an `https` source; `Media::from(attachment_ref)` (or passing the
  `AttachmentRef` directly) is `attachment://<name>`. `.as_str()` gives the URL and
  `.attachment()` the `AttachmentRef` it came from, if any. It serializes as exactly
  `{"url": …}`.
- Small on mobile: never put information-bearing text in it.

## 5. Media Gallery: `MediaGallery`

- **Where:** top level or Container.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) 1–10 items (CV2-10); item `media.url` https or `attachment://` (ATT-05);
  item `description` (alt) ≤ 1024 (CV2-10). Cost 1 regardless of items — the cheapest
  way to show visual data. 1 item = full width; 2–4 = grid. Images and video.
- **Builder:** `MediaGallery::new([MediaItem; N])`; `MediaItem::new(media).alt(..)
  .spoiler(..)` (items carry no type or id). `N` is bounded by
  `Arity<N>: GalleryItems` (1..=10): 0 or 11 items fail `cargo check` with
  `[CV2-10] a Media Gallery holds 1 to 10 items`. For a count known only at runtime use
  `MediaGallery::try_new(items, &limits)` (`Err(UiError::Limits { .. })`, CV2-10). JSON
  type 12.

```rust
MediaGallery::new([MediaItem::new(card_ref).alt(s.profile.card_alt(&name))])
```

## 6. File: `File`

- **Where:** top level or Container.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) `file` is **only** `attachment://<name>` (CV2-12); one file per File
  component. `name`/`size` are response-only. Cost 1.
- **Builder:** `File::new(attachment_ref).spoiler(bool)`; `.attachment()` reads the
  ref. It takes only an `AttachmentRef`, which only `AttachmentSet::add` returns, so a
  dangling or hand-written `attachment://` name does not compile (E0308). JSON type 13.

### Attachments: `Attachment`, `AttachmentSet`, `AttachmentRef`

```rust
use noctis::ui::{Attachment, AttachmentSet, File, MediaItem, MediaGallery};

let mut files = AttachmentSet::new();
let card = files.add(Attachment::new(png_bytes, "png")?.alt("Profile card"));
let body = MediaGallery::new([MediaItem::new(card.clone())]);
let raw = File::new(files.add(Attachment::new(csv_bytes, "csv")?.content_type("text/csv")));
```

- `Attachment::new(bytes, ext)` names the file by its content:
  `<first 16 hex digits of blake3(bytes)>.<ext>`; `Attachment::with_digest(bytes,
  digest, ext)` uses a 32-byte digest you already have; `Attachment::rendered(bytes,
  digest, ext)` marks a rendered image (`AttachmentOrigin::Rendered`, otherwise
  `AttachmentOrigin::Plain`). `ext` is 1–10 ASCII letters or digits, kept verbatim;
  anything else is `UiError::InvalidExtension { ext }`.
- Setters `.alt(text)` and `.content_type(mime)`; getters `name()`, `bytes()`,
  `alt_text()`, `mime()`, `origin()`.
- `AttachmentSet::new()`, `.add(att) -> AttachmentRef` (the same name twice keeps the
  first one and returns equal refs), `.get(name)`, `.iter()` (name order), `.len()`,
  `.is_empty()`.
- `AttachmentRef` has no public constructor: `.name()`, `.url()`
  (`attachment://<name>`), `.origin()`; `Clone + Eq + Hash`.
- In a render function, `files.add(att) -> AttachmentRef` on the message's
  `AttachmentSet`, then `V2Message::attachments(files)`.
- `V2Message::used_attachments() -> impl Iterator<Item = &Attachment>`: the set
  members whose `attachment://<name>` appears at a File, Thumbnail or MediaItem site
  (through an `AttachmentRef` or a hand-written `Media::url("attachment://<name>")`),
  each once, in name order. Only these are sent; an unreferenced member is dropped
  (one `debug!` on target `noctis::responder`).
- **Sending (automatic, FR-30).** The Responder uploads each used file once as
  `multipart/form-data` (`payload_json`, then `files[0]`, `files[1]`, … over the new
  files only, ATT-04), with entry `{"id": n, "filename": name}` plus `"description"`
  when `Attachment::alt` is set. On an edit (`update`, `PATCH @original`, follow-up
  edits, channel edits) a file the target message already holds (same content name) is
  kept by id (`{"id": "<snowflake>"}`, not re-uploaded) and files no longer used are
  dropped; edit bodies always carry the `attachments` keep-list (MSG-08), create bodies
  only when a file is used. The same View re-rendered with the same bytes uploads
  nothing. `Attachment::mime` is not sent (Discord reads the extension).
- **Upload Budget (ATT-01..03)**, checked before any request, in Strict and Lenient
  alike: each new file ≤ 20 MiB (`Limits::attachment_bytes`; an interaction's
  `attachment_size_limit` wins), the whole request (`payload_json` + new files) ≤ 25
  MiB, ≤ 10 used attachments (kept ones count, with size 0). A miss is
  `ResponderError::Invalid(UiError::Limits { .. })` and nothing is sent.
- **Rendered images need alt text (ATT-06, FR-30, UX-DR2).** A Thumbnail or MediaItem
  showing an `Attachment::rendered(..)` must have its own `.alt(..)` (the site's alt;
  `Attachment::alt` does not count). Without one (`None`, `""`, whitespace): Strict
  `validate` fails with `[ATT-06]` at `<site path>.description` (help: add `.alt(..)`
  to the Thumbnail or MediaItem); Lenient and `cost` report `LimitWarning::MissingAlt`.
  For a fixed alt text use `alt!("Profile card")` (an `AltText`, ≤ 1024 checked at
  `cargo check`; `AltText::try_new(text, &limits)` at runtime).

## 7. Separator: `Separator`

- **Where:** top level or Container.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) `divider` bool; `spacing` 1 small / 2 large only (CV2-13, the `Spacing`
  enum has no other value). Cost 1.
- **Builder:** `Separator::new()` = `Separator::default()` = `Separator::small()`
  (divider, small); `Separator::large()`; `.divider(false)`;
  `.spacing(Spacing::Small | Spacing::Large)`. All `const`. JSON type 14.
- Large before major sections, small inside.

## 8. Action Row: `ActionRow`

- **Where:** top level or Container (in modals: deprecated; Noctis never emits it).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) 1–5 buttons **or** exactly 1 select, never mixed (CV2-06).
  `ActionRow::buttons` is bounded by `Arity<N>: RowButtons` (1..=5): an empty array or
  6 buttons fail `cargo check` with `[CV2-06] an Action Row holds 1 to 5 buttons`. For a
  count known only at runtime use `ActionRow::try_buttons(buttons, &limits)` (any
  `IntoIterator` of one `RowItem` type; `Err(UiError::Limits { .. })`, CV2-06). Cost 1 +
  children. Rows wrap on mobile beyond ~3 labelled buttons; keep
  labels short.
- **Builder:** `ActionRow::buttons([b1, b2, …])` (an array of one button type; mix kinds
  by converting each with `Button::from(..)`), `ActionRow::select(select)`. JSON type 1.

```rust
ActionRow::buttons([
    Button::from(Save { guild }.build().style(ButtonStyle::Primary)),   // label from the defaults
    Button::from(LinkButton::new("https://example.com/help").label("Help")),
])
```

An optional button: build the row in each branch (`match extra { Some(b) =>
ActionRow::buttons([a, b]), None => ActionRow::buttons([a]) }`), or collect a `Vec<Button>`
and call `ActionRow::try_buttons(buttons, &limits)?`.

## 9. Buttons (type 2)

| Field | Limit | Rule |
| --- | --- | --- |
| `style` | 1 Primary, 2 Secondary, 3 Success, 4 Danger, 5 Link, 6 Premium | CV2-08 |
| `label` | ≤ 80 (guideline: ≤ 34 with emoji, 38 without) | CV2-08 |
| `emoji` | `Emoji::unicode("🔄")`, `Emoji::custom(id, "name", animated)` | — |
| `custom_id` | styles 1–4 only; built from the Route, never by hand | CID-01..07 |
| `url` | Link only, ≤ 512 | CV2-08 |
| `sku_id` | Premium only | CV2-08 |
| `disabled` | bool | — |

Each kind is its own type (CV2-08), so a Link button cannot get a `custom_id` and a
Premium button cannot get a label. `Button` is the opaque "any button" they all convert
into (`Button::from(x)` / `.into()`), for mixed rows. Never in modals. Guidelines: one
Primary per group; equal-weight actions all Secondary; Danger only for destructive
actions. Tab rows, prev/next and Back/Next/Cancel are ordinary Route buttons you build
in your render function (`views-and-recipes.md` §3).

### 9a. Route button: `RouteButton`

- **Where:** Action Row (`RowItem`) or Section accessory (`SectionAccessory`).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) styles 1–4; label ≤ 80 (CV2-08); `custom_id` ≤ 100, proven by the Route
  derive (CID-01).
- **Builder:** `MyRoute { .. }.build()` on a `#[derive(Button)]` struct → `RouteButton<MyRoute>`, starting from the struct's `defaults(..)`; `.style(ButtonStyle::
  Primary | Secondary | Success | Danger)` (default Secondary), `.label(..)` (a literal or
  a `MessageKey`, translated in the response locale before sending),
  `.emoji(Emoji::unicode("🛒"))`, `.disabled(bool)`, or several at once with
  `.overrides(ButtonOverrides { label: Some("Again".into()), disabled: Some(true),
  ..Default::default() })` (the later call wins). A click reaches the Route's handler
  with the decoded Payload as `self`. The defaults live on the struct
  (`#[button("id", defaults(style = .., label = "…" | keys::X | emoji = "…", disabled = ..))]`;
  `style` and one of `label`/`emoji` are required); a `defaults` text is a string literal
  (fixed text, checked at `cargo check`) or a `keys::` path (a locale key the Responder
  resolves in the response locale and validates after resolving; a path that is not a
  `MessageKey` is rustc's `E0308`; `i18n.md` §2). A literal label can be
  checked at `cargo check`
  with `label!("Save")` (a `Label`, also usable in a `const`); runtime text (localized
  labels included) is passed as is or checked with `Label::try_new(text, &limits)`.

```rust
ToggleModule { guild, module, enable: !enabled }
    .build()                                  // defaults(style = Success, label = keys::COMMON_ENABLE)
    .label(s.common.disable())                // override for this render
    .style(ButtonStyle::Danger)
```

### 9b. Link button: `LinkButton`

- **Where:** Action Row or Section accessory.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) `url` 1–512 (CV2-08), label ≤ 80; no `custom_id`, no interaction.
- **Builder:** `LinkButton::new(url)`, `.label(..)`, `.emoji(..)`, `.disabled(bool)`.

### 9c. Premium button: `PremiumButton`

- **Where:** Action Row or Section accessory.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) needs `sku_id`; no label, emoji, URL or `custom_id` (CV2-08: the type has
  no such setters).
- **Builder:** `PremiumButton::new(sku_id)` (`Id<SkuMarker>`), `.disabled(bool)`; `const`.

## 10. Select menus (types 3, 5, 6, 7, 8) — message variants

Every select carries the per-render nonce (FR-25). A select sits alone in its row:
`ActionRow::select(route.build())`. `defaults(..)` needs `placeholder`. The shared fields below are `defaults(..)`
keys on the struct and per-render overrides.

Shared fields of all five:

| Field | Builder | Limit | Rule |
| --- | --- | --- | --- |
| `placeholder` | `.placeholder(..)` | ≤ 150 | CV2-09 |
| `min_values` | `.min_values(n: u8)` | 0–25 (default 1) | CV2-09 |
| `max_values` | `.max_values(n: u8)` | 1–25 (default 1), ≥ min | CV2-09 |
| `disabled` | `.disabled(bool)` | messages only | CV2-09 |

- **Cost:** 2 (row + select). Options don't count.
- After every re-render, selects show the render function's defaults (nonce, FR-25): each send
  draws a nonce that differs from every nonce in the clicked interaction (the select's own id
  and the message's selects) and from the handler run's last eight sends, so the client never keeps
  the user's old local selection. Always set `default`/`default_*` to the current value
  (visible active state).
- Empty list: render a disabled select with one explanatory option, or an empty-state
  Text Display, never nothing.
- More than 25 items: page them (25 options per page with prev/next, `views-and-recipes.md` §3.2).
- A `derive(Choices)` enum fills a String Select through `defaults(options = SomeChoicesEnum)`
  on `#[derive(StringSelectMenu)]` (§10a). Map `X::ALL` by hand only when the options change
  per render, and localize each label's text.
- Selected values in the handler: `ctx.values()` returns a `SelectedValues` typed by the
  select's kind: `.strings()` (every raw value, any kind), `.users()`, `.roles()` and
  `.channels()` (ids in the order sent, duplicates kept); a Mentionable Select's values
  are split into `.users()` and `.roles()` by the interaction's resolved data. A value
  that is not an id stays in `.strings()` only. Noctis 0.1 has no `ctx.resolved()` on a
  component context; load the entities with `ctx.cache()` from the ids.

### 10a. String Select: `StringSelect`

- **Where:** alone in an Action Row (`RowSelect`).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) ≤ 25 options; option label/value/description ≤ 100; `default` count
  within [min, max] (CV2-09). Zero options build (the validator checks them).
- **Builder:** `route.build()` on a `#[derive(StringSelectMenu)]` struct. With
  `defaults(options = SomeChoicesEnum)` it returns a `StringSelect` holding the enum's
  options (`.options(..)` replaces them per render). Without it, it returns a
  `StringSelectBuilder<X>` that must get `.options(..)` (any `IntoIterator<Item =
  SelectOption>`, so a `Vec` works) before it is placed: placing it without options is
  E0277 ``[CV2-09] this String Select has no options: call `.options(..)` or declare
  `defaults(options = …)` ``. `StringSelectBuilder` has `.placeholder(..)`,
  `.min_values(n)`, `.max_values(n)`, `.disabled(bool)`, `.default_values(values)` and
  `.options(..) -> StringSelect` (no `.overrides`: give options first).
  `SelectOption::new(label, value).description(..).emoji(..).default(true)`.
  `.default_values(["b"])` (option values) replaces every option's own `default` flag, in
  either order with `.options(..)`; a value no option has is `[CV2-09]` at
  `…default_values[i]` in both modes. `.overrides(StringSelectOverrides { placeholder,
  min_values, max_values, disabled, options, default_values, ..Default::default() })` sets
  several fields at once.
  Options given empty at runtime fail the validator with `[CV2-09]` in every limit mode.
  JSON type 3. There is no `try_string_select`; check a runtime-built select with
  `noctis::ui::validate` (§16).

```rust
#[derive(StringSelectMenu, Debug, Clone, Copy)]
#[string_select("shop.size", defaults(placeholder = keys::SHOP_SIZE))]
pub struct Size { pub item: u32 }

Size { item }.build().options([
    SelectOption::new("Small", "s").default(current == SizeKind::Small),
    SelectOption::new("Large", "l").default(current == SizeKind::Large),
])
```

### 10a2. String Select without options yet: `StringSelectBuilder`

- **Where:** nowhere until it has options: `.options(..)` turns it into a `StringSelect`
  for an Action Row; placing it as is is E0277 `[CV2-09] this String Select has no
  options` (the `OptionsGiven` bound).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as String Select once given options; setters keep the typestate.
- **Builder:** `X.build()` on a `#[derive(StringSelectMenu)]` struct whose `defaults(..)`
  have no `options`; `.placeholder(..)`, `.min_values(n)`, `.max_values(n)`,
  `.disabled(bool)`, then `.options(iter) -> StringSelect`.

### 10b. User Select: `UserSelect`

- **Where:** alone in an Action Row.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) shared fields; `default_values` count within [min, max] (CV2-09).
- **Builder:** `route.build()` on a `#[derive(UserSelectMenu)]` struct, `.default_users(ids)`
  (any `IntoIterator<Item = Id<UserMarker>>`, so an `Option` works; appends, duplicates
  count once), `.default_values(ids)` (replaces), `.overrides(UserSelectOverrides { .. })`.
  JSON type 5.

### 10c. Role Select: `RoleSelect`

- **Where:** alone in an Action Row.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as User Select.
- **Builder:** `route.build()` on a `#[derive(RoleSelectMenu)]` struct, `.default_roles(ids)`.
  JSON type 6.

### 10d. Mentionable Select: `MentionableSelect`

- **Where:** alone in an Action Row.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as User Select.
- **Builder:** `route.build()` on a `#[derive(MentionableSelectMenu)]` struct,
  `.default_users(ids)`, `.default_roles(ids)`
  (appended in call order). JSON type 7.

### 10e. Channel Select: `ChannelSelect`

- **Where:** alone in an Action Row.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as User Select.
- **Builder:** `route.build()` on a `#[derive(ChannelSelectMenu)]` struct,
  `.default_channels(ids)`, `.channel_types([ChannelType::GuildText])` (twilight's
  `ChannelType`; none = every type; also a `defaults(channel_types = [GuildText, ..])`
  key). JSON type 8.

## 11. Raw escape hatch (FR-31)

Use the typed components whenever one exists. The raw hatch is for a component type
Noctis does not model yet (a new Discord component) and for endpoints twilight does not
model (§11b).

### 11a. Raw component: `RawComponent`

```rust
use noctis::ui::{Components, Container, RawComponent, TextDisplay, V2Message};
use serde_json::json;

let poll = RawComponent::new(json!({"type": 99, "question": "Lunch?"}))?; // UiError::Limits on bad JSON
let view = V2Message::new(
    Components::new()
        .push(TextDisplay::new("Vote"))
        .push(poll)                                   // top level
        .push(Container::new().push(RawComponent::try_from(json!({"type": 98}))?)), // or in a Container
);
// [{"type":10,"id":1,"content":"Vote"},{"type":99,"question":"Lunch?"},{"type":17,"id":2,"components":[{"type":98}]}]
```

- **Where:** the top level of a message or inside a Container (it implements
  `TopLevel` and `ContainerChild`), also as a `components!` `{hole}` there. Never a
  Section accessory, an Action Row item or a modal input: those fail `cargo check`
  (E0277, `[CV2-05]`/`[CV2-07]`/`[MOD-03]` messages).
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) CV2-03: counts `raw.count()` components (every component object it holds,
  itself included) in `validate`, `cost` and `Budget`; a `{hole}` counts 1 at
  expansion and the real count at run time. CV2-14: an `id` key on any walked component
  object is rejected (``remove `id`; Discord numbers raw components itself``); Noctis
  never numbers a raw node and the ids around it stay consecutive. CV2-16: every walked
  node (the root, then only its `components` array, `accessory` and `component`
  children) must be a JSON object with an integer `type` ≥ 1, `components` must be an
  array, and nesting is at most 32 levels. Other keys (`emoji`, `options`,
  `default_values`, …) are not walked. Every violation is collected in pre-order, with
  paths like `components[0].accessory.id`; the validator names a raw node
  `opaque[i]` (`container[0].opaque[2]`).
- **Builder:** `RawComponent::new(json!(..))?` or `RawComponent::try_from(value)?`;
  read back with `.as_json()`, `.into_json()`, `.component_type()` and `.count()`. There
  is no mutation API: the JSON is sent verbatim (no id injection, no truncation, no key
  changes).

Stability caveats:

- Noctis checks only the shape above: no placement rules and no field checks inside a raw
  node. Discord may change, reject (400) or ignore component types Noctis does not
  model; such a send fails at Discord, not before.
- `custom_id`s inside raw JSON are not checked (CID-02, select freshness) and no Route
  handles them unless you encode them yourself.
- Raw text is not counted toward CV2-04, and Lenient mode never truncates it.
- Every `attachment://<name>` string inside a raw node is tracked like a `Media` URL: a
  name of the message's `AttachmentSet` is uploaded or kept (§12), any other name is
  ATT-05 at `opaque[i].url`.

Receive side:

- `ctx.unknown_components()` on `ComponentCtx` gives the clicked message's components
  Noctis does not model (types other than 1, 2, 3, 5-14 and 17, or malformed nodes),
  verbatim and in pre-order; an unknown node is taken whole. On `ModalSubmitCtx` it gives
  the submit's unknown components. Empty when there are none.
- `noctis::ui::unknown_components(&components)` does the same walk over any raw
  `components` array you hold.
- A click on a message whose components twilight cannot parse still reaches its handler
  (the gateway re-parses it without them).
- An update or edit never re-sends the received components: `ctx.update(view)`, a
  returned View and `defer_update` + `edit_reply` send only the View you give. Keep a
  raw node by putting it into the View again.

### 11b. Raw REST requests

```rust
use noctis::http::Method;                              // RawHttp is in the prelude

let body = serde_json::json!({"content": "hi"});
let sent = ctx.http().raw(Method::Post, format!("channels/{id}/messages")).json(&body).await?;
let msg: serde_json::Value = sent.json()?;            // RawResponse: status(), header(name), bytes()
```

- `ctx.http()` (every context) and `app.http()` are the App's one twilight client;
  `.raw(method, path)` goes through it, so it is rate limited (or proxied, then the
  proxy limits), uses the configured timeout and the bot token.
- Path rule: the part after `/api/v10/`, query included (`channels/1/messages?limit=2`);
  no leading `/`, no empty, `.` or `..` segment, not starting with `api`, printable
  ASCII only without `#` or `\` (percent-encode the rest, e.g. an emoji
  `reactions/%F0%9F%91%8D/@me`). A bad path is `RawHttpError::InvalidPath` and nothing
  is sent.
- Bodies: `.json(&value)`, `.body(bytes)` (JSON bytes) or `.form(Form)` (multipart,
  `noctis::twilight::http::request::Form`); the last one set wins.
  `.without_token()` for webhook and interaction-token routes.
- `.raw(..)` returns a `RawRequest` (nothing is sent until `.await`); a 2xx answer is a
  `RawResponse`: `.status()`, `.header(name)` (case-insensitive, first value),
  `.bytes()`, `.into_bytes()` (`bytes::Bytes`) and `.json::<T>()`.
- Errors (`RawHttpError`, converts into `HandlerError` with `?`): `InvalidPath`,
  `Encode`, `RateLimited(RateLimited)` for a 429 (never retried; RL-04),
  `Status { status, body }` for any other non-2xx, `Body`, `Decode` (from
  `.json()`), `Http` (timeout, transport).
- The Responder never sees raw requests: never answer an interaction (callbacks,
  `@original`, follow-ups) through `.raw(..)`; use `ctx.respond`/`ctx.update`.

## 12. Messages: `V2Message`, `LegacyMessage`, flags and mentions

```rust
use noctis::ui::{
    ActionRow, CallbackFlags, Components, CreateFlags, EditFlags, LegacyMessage, LinkButton,
    Mentions, MessageFormat, Roles, TextDisplay, UiError, Users, V2Message,
};

let msg = V2Message::new(Components::new().push(TextDisplay::new("Saved")))
    .mentions(Mentions::none().users(Users::All));          // default: pings nobody
let body = msg.callback_body(CallbackFlags::EPHEMERAL);     // a MessageBody<'_>
// {"flags":32832,"components":[{"type":10,"id":1,"content":"Saved"}],"allowed_mentions":{"parse":["users"]}}

let notice = LegacyMessage::new("Plain text notice")
    .rows([ActionRow::buttons([LinkButton::new("https://example.com").label("Docs")])]);
assert_eq!(notice.edit_body(MessageFormat::V2, EditFlags::empty()).unwrap_err(), UiError::V2ToLegacy);
```

**`V2Message`** (the message of every View and every interactive surface):

- `V2Message::new(components)`, `V2Message::from(components)`; no `Default`.
- Setters: `.attachments(set)` replaces its `AttachmentSet` (the files the tree
  references; bodies never carry it), `.mentions(Mentions)`.
- Getters: `.components()`, `.attachment_set()`, `.allowed_mentions()`.
- No `content`, `embeds`, `poll`, `sticker_ids` or `shared_client_theme` exist on it
  (CV2-02, compile error E0599).

**`LegacyMessage`** (the only legacy path: plain text only, no embeds, polls or
stickers). Never use it for interactive UI or dashboards (UD-1); use it for plain
notices.

- `LegacyMessage::new(content)` (`content` length, MSG-01, is checked by
  `noctis::ui::validate`, §16; `content!("…")` checks a literal at `cargo check`); `.rows([ActionRow; N])` replaces the rows, N from 0 to 5;
  `.mentions(Mentions)`; no `Default`.
- Getters: `.content_text()`, `.allowed_mentions()`, `.row_count()`.
- CV2-15: `.rows` is bounded by `Arity<N>: LegacyRows` (implemented for `Arity<0>` to
  `Arity<5>`). Six rows fail with
  ``[CV2-15] a LegacyMessage holds at most 5 Action Rows`` (note: `` `Arity<6>` is the
  number of rows; send the rest in another message, or build a V2Message ``).

**Bodies.** Flags are chosen per endpoint when you ask for the body, so one message can
be sent, replied or edited. Each body method returns a `MessageBody<'_>` (borrowed,
`#[must_use]`, `Serialize`) that core's Responder sends; the Responder picks the body
for the endpoint and adds the `attachments` entries and `files[n]` parts (§6). You rarely call these yourself.

| Method | Endpoints | Flag set and its consts |
| --- | --- | --- |
| `create_body(CreateFlags)` | Create Message, Execute Webhook | `CreateFlags::SUPPRESS_EMBEDS` (4), `CreateFlags::SUPPRESS_NOTIFICATIONS` (4096) |
| `callback_body(CallbackFlags)` | callback type 4 data, follow-up messages | `CallbackFlags::SUPPRESS_EMBEDS` (4), `CallbackFlags::EPHEMERAL` (64), `CallbackFlags::SUPPRESS_NOTIFICATIONS` (4096) |
| `edit_body(MessageFormat, EditFlags)` | Edit Message, Edit Original Interaction Response, Edit Webhook Message, callback type 7 (`UPDATE_MESSAGE`) data | `EditFlags::SUPPRESS_EMBEDS` (4) |
| `webhook_edit_body(EditFlags)` (V2 only) | Edit Webhook Message (what `noctis::webhook` sends) | `EditFlags::SUPPRESS_EMBEDS` (4); always clears the legacy fields, whatever the old format (WH-03) |

- Every flag set is `Copy + Default + Eq + Hash` with `empty()`, `union(other)` (const),
  `|`, `|=`, `contains(other)`, `is_empty()` and `bits()`. Only the consts above exist
  (MSG-07, INT-09): no `IS_COMPONENTS_V2` (CV2-01), no `IS_VOICE_MESSAGE`, no
  `from_bits`, `all` or `remove`. `const F: CreateFlags =
  CreateFlags::SUPPRESS_EMBEDS.union(CreateFlags::SUPPRESS_NOTIFICATIONS);` works.
- V2 bodies always send `flags = 32768 | your bits` (`bits()` never includes 32768).
  Legacy bodies never set 32768; `create_body`/`callback_body` leave `flags` out when
  empty, `edit_body` always sends it (so an unset `SUPPRESS_EMBEDS` is cleared).
- Every body sends `allowed_mentions`. Key order: V2 `flags`, legacy clears,
  `components`, `allowed_mentions`; legacy `flags`, `content`, `components`,
  `allowed_mentions`. Legacy rows carry no numeric `id`s.
- An unresolved Route `custom_id` makes serializing the body an error; core resolves
  them before sending.

**Edits and `MessageFormat`.** An edit names the format of the message it replaces:
`MessageFormat::{Legacy, V2}`, read from its flags with
`MessageFormat::from_flags(bits)` (V2 when bit 32768 is set).

- `V2Message::edit_body(MessageFormat::Legacy, ..)` turns a legacy message into V2 and
  clears its legacy fields in the same request:
  `{"flags":32768,"content":null,"embeds":[],"poll":null,"sticker_ids":[],"components":[..],"allowed_mentions":{..}}`.
- `LegacyMessage::edit_body(MessageFormat::V2, ..)` returns `Err(UiError::V2ToLegacy)`
  ("a Components V2 message cannot be edited back into a legacy message (CV2-01: the
  IS_COMPONENTS_V2 flag cannot be removed)"); with `MessageFormat::Legacy` it is `Ok`.

**Mentions** (MSG-03, MSG-05). `Mentions::default()` = `Mentions::none()` pings nobody
(`{"parse":[]}`), even though mentions in the text still render. `.mentions` takes a
`Mentions`, never an `Option`. Opt in per type:

- `.users(Users)` and `.roles(Roles)`, each `None` (default), `All` or `Only(ids)`;
  `Users::only([id, ..])` / `Roles::only([id, ..])` build `Only` from any iterator of
  `Id`s. `All` takes no ids, so `parse` plus explicit ids cannot be built (MSG-03).
- `.everyone(bool)` (`@everyone`/`@here`), `.replied_user(bool)`.
- Getters: `.user_mentions()`, `.role_mentions()`, `.pings_everyone()`,
  `.pings_replied_user()`.

| Mentions | JSON |
| --- | --- |
| `Mentions::none()` | `{"parse":[]}` |
| `.users(Users::All)` | `{"parse":["users"]}` |
| `.users(Users::only([Id::new(1), Id::new(2)]))` | `{"parse":[],"users":["1","2"]}` |
| `.users(Users::Only(vec![]))` | `{"parse":[]}` |
| `.users(Users::All).roles(Roles::only([Id::new(5)])).everyone(true)` | `{"parse":["users","everyone"],"roles":["5"]}` |
| `.replied_user(true)` | `{"parse":[],"replied_user":true}` |

`parse` is always ordered users, roles, everyone. The 100-ids-per-list limit (MSG-04)
is checked by `noctis::ui::validate` (§16).

## 12b. Static messages for foreign webhooks: `StaticMessage`

A webhook the App does not own (`ForeignWebhook`, see `responder.md` §7) cannot send
interactive components: Discord drops them or rejects the request (WH-02). Its `send` and
`edit` take only an `IntoStaticMessage`: a `StaticMessage` or a `StaticComponents`. The
static builders mirror the V2 ones but accept only non-interactive children, so a Route
button or a select in a foreign webhook message fails `cargo check` with `[WH-02]`.

```rust
use noctis::ui::{
    LinkButton, StaticActionRow, StaticComponents, StaticContainer, StaticMessage,
    StaticSection, TextDisplay, Thumbnail,
};

let msg = StaticMessage::new(
    StaticComponents::new().push(
        StaticContainer::new()
            .push(StaticSection::new(
                [TextDisplay::new("## Release 1.4")],
                Thumbnail::new("https://example.com/logo.png").alt("Logo"),
            ))
            .push(StaticActionRow::buttons([
                LinkButton::new("https://example.com/notes").label("Notes"),
            ])),
    ),
);
```

- `StaticComponents::new().push(..)` takes `StaticContainer`, `StaticSection`,
  `StaticActionRow`, `TextDisplay`, `MediaGallery`, `File` or `Separator`
  (`StaticTopLevel`); `.len()`, `.is_empty()`; `Components::from(static_components)`.
- `StaticMessage::new(components)` / `StaticMessage::from(components)`; `.attachments(set)`,
  `.mentions(Mentions)` as on `V2Message`; `.as_v2()` borrows the `V2Message`;
  `V2Message::from(static_msg)` (or `V2Message::from(static_components)`) converts back (every static message is a valid V2 message).
- A tree built at runtime: `StaticMessage::try_from(v2_message)` walks it at any depth,
  `RawComponent` JSON included, and returns `Err(UiError::Limits { violations })` with
  every WH-02 violation (paths as in §16: `row[0].button[1]`,
  `container[0].section[2].accessory`, `opaque[1].components[0]`), never a partial message.
- Placement traits (sealed): `StaticTopLevel`, `StaticContainerChild`, `StaticAccessory`,
  `StaticRowButton`, `IntoStaticMessage`. Each error starts with `[WH-02]` and says which
  types fit.
- An owned webhook (`OwnedWebhook`) takes any `V2Message`, interactive or not; use the
  static builders only for foreign webhooks.

### Static Container: `StaticContainer`

- **Where:** top level of a `StaticComponents`. Never nested.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as `Container` (§1): CV2-03 counts, accent colour, spoiler; WH-02 for its
  children (`StaticContainerChild`: `StaticSection`, `StaticActionRow`, `TextDisplay`,
  `MediaGallery`, `File`, `Separator`). A plain `Section` or `ActionRow` is a compile
  error, because it may hold interactive components.
- **Builder:** `StaticContainer::new().push(child)`, `.accent(Rgb)`, `.spoiler(bool)`;
  `Container::from(static_container)`. JSON type 17.

### Static Section: `StaticSection`

- **Where:** top level of a `StaticComponents` or a `StaticContainer` child.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) as `Section` (§3): 1–3 Text Displays (CV2-07); the accessory is a
  `Thumbnail` or a `LinkButton` (`StaticAccessory`, WH-02). A `RouteButton`,
  `PremiumButton` or `Button` accessory fails with
  ``[WH-02] `X` is not a static Section accessory``.
- **Builder:** `StaticSection::new([TextDisplay; N], accessory)`;
  `Section::from(static_section)`. JSON type 9.

### Static Action Row: `StaticActionRow`

- **Where:** top level of a `StaticComponents` or a `StaticContainer` child.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) 1–5 `LinkButton`s (CV2-06 arity, `StaticRowButton`, WH-02). No selects and
  no buttons with a `custom_id`: ``[WH-02] `X` is not a static row button``.
- **Builder:** `StaticActionRow::buttons([LinkButton; N])`; `ActionRow::from(static_row)`.
  JSON type 1.

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
| Selects 3,5–8 | no | no | — | alone | inside Label, as the separate `Modal*Select` types (no `disabled`) |
| Label 18 (`ModalLabel`) | — | — | — | — | top level (`ModalTopLevel`) |
| Text Input 4, File Upload 19, Radio 21, Checkbox Group 22, Checkbox 23 | — | — | — | — | inside Label only (`ModalInput`); see `modals.md` |

## 14. `components!`: literal trees checked at compile time

For a fixed-shape message, write the tree with `noctis::components!` (in the prelude;
FR-78). It expands to the builders above and yields a `Components`; when the literal
tree breaks a rule, `cargo check` fails with the rule id, spanned on the offending node
or literal.

```rust
use noctis::prelude::*;
use noctis::ui::{ButtonStyle, V2Message};

let body = components! {
    container(accent = rgb!(0x5865_F2)) [
        text("### Server settings"),
        separator(spacing = small),
        section [ text("Logs go to the channel you pick."), text(current); accessory = SetLogs { guild }.build() ],
        gallery [ item(card_ref, alt = "Profile card") ],
        row [ Save { guild }.build().style(ButtonStyle::Primary), link(url = "https://example.com/help", label = "Help") ],
        row [ Pick { guild }.build() ],          // placeholder from the struct's defaults
        ..extra_rows,                 // spread: any IntoIterator of Container children
    ],
    {footer},                         // hole: one component you built elsewhere
};
V2Message::new(body)
```

### Grammar

```text
components! { node,* }
node      := container | section | text | gallery | file | separator | row | {hole} | ..spread
container := container [ "(" accent = expr ")" ] "[" node,* "]"
section   := section "[" text,* ";" accessory = (thumbnail | button | {hole}) "]"
text      := text "(" string-literal | expr ")"
thumbnail := thumbnail "(" media [, alt = string-literal | expr] ")"
gallery   := gallery "[" item,* "]"             (items or {hole}s)
item      := item "(" media [, alt = string-literal | expr] ")"
file      := file "(" attachment_ref ")"
separator := separator [ "(" [spacing = small | large] [, divider = expr] ")" ]   (both optional, any order, each once)
row       := row "[" (button | {hole}),* "]" | row "[" select "]"
button    := route.build() [.setter(..)]* | link(url = "…", label = "…") | premium(sku)
select    := route.build() [.setter(..)]*
hole      := "{" expr "}"                       one already-built component (block statements allowed)
spread    := ".." expr                          IntoIterator of components; top level or container only
```

- `media` is a string literal (an `https` URL, becomes `Media::url(..)`) or any
  expression convertible into `Media` (an `AttachmentRef`, a `Media`).
- A Route component is any expression whose method chain contains `.build()`; its kind comes from the struct's
  derive: the receiver is the Route value and the calls after it
  (`.style(..)`, `.label(..)`, `.placeholder(..)`, `.max_values(..)`, `.disabled(..)`,
  `.emoji(..)`, …) are re-applied in order.
- `link` needs literal `url` and `label`, in that order. There is no `button_with`; build a
  configured button outside the macro and pass it as a `{hole}`.
  Spreads in a `row`, `section` or `gallery` are a syntax error:
  build a dynamic row with `ActionRow::try_buttons` and pass it as a `{hole}`.
- A `{hole}` inside `row [ .. ]` is a button; a pre-built select there fails with E0277
  `[CV2-05]`. Pass it as a node-level hole instead: `{ActionRow::select(sel)}`.
- Syntax errors (unknown keyword such as `box [ ]`, a non-component expression at a
  node position, an unknown or repeated argument) carry no rule id.

### Expansion

Typed builder calls under `::noctis::__private::ui` (no import needed): `container` is
`Container::new().accent(..).push(..)…`, `section` `Section::new([..], accessory)`,
`row` `ActionRow::buttons([..])` or `ActionRow::select(..)`, a spread a `for` loop of
`push`. Holes and spreads therefore still go through the placement traits: a misplaced
hole is the usual E0277 `[CV2-05]` / `[CV2-07]`. Every expression is evaluated exactly
once, in source order; the macro's own locals never clash with yours.

### What it checks

Against `Limits::default()`, with the same `noctis-rules` functions and paths as the
runtime validator:

| Rule | Check |
| --- | --- |
| CV2-03 | ≤ 40 components at every depth, accessories included; a hole counts 1, a spread 0; the error is spanned on the node that crosses 40 and has one `= note: kind[i]: n` per top-level node |
| CV2-04 | ≤ 4000 `discord_len` across literal `text("…")` (top level, Container, Section); non-literal text counts 0; spanned on the literal that crosses 4000 |
| CV2-05 | `container` in `container`, `thumbnail` outside an accessory, a button or select outside a `row`, `text` in a `row`, `item` outside a `gallery` |
| CV2-06 | a `row` with 0 or more than 5 items, or a select next to anything |
| CV2-07 | a `section` with 0 or more than 3 texts, a non-`text` before the `;`, an accessory that is not a button, Thumbnail or hole |
| CV2-08 | literal button labels (`.button("…")`, `.label("…")`, `link(label = "…")`) ≤ 80; literal link URLs 1–512 |
| CV2-09 | literal `.placeholder("…")` ≤ 150; literal `min_values` 0–25, `max_values` 1–25, `min ≤ max`; an array-literal `string_select([..])` has 1–25 elements |
| CV2-10 / CV2-11 | `gallery` with 0 or more than 10 items; literal `alt = "…"` ≤ 1024 |
| CV2-13 | an integer literal `accent = 0x…` above `0xFFFFFF`; `spacing` other than `small` / `large` |
| CV2-17 | literal media URLs 1–2048 |
| CID-02 | two components whose Route value is token-identical and literal (see below) |

Every violation of one invocation is reported in one expansion (in validator order,
then CID-02), and the expansion is then an empty `Components`. A rule error reads like
the runtime one: `[CV2-08] row[0].button[0].label: limit at most 80, actual 81`.

**CID-02 scope.** Only receivers fixed by their tokens are compared: a struct literal
without `..rest` whose every field is a literal (`Press { n: 1 }`), a call of an
uppercase path with literal arguments (`Page(1)`), or a unit path (`Refresh`). Two such
receivers with identical tokens (button or select) fail with
``[CID-02] duplicate custom_id: `Press { n : 1 }` is already used at row[0].button[0]``
on the second and ``[CID-02] first use of the duplicate custom_id …`` on the first.
`Press { n }`, `Press { n: x }`, variables and `1` vs `1u8` are not compared.
A `by_ref` Route written twice with equal literal values is rejected too (at runtime equal
slots share one Ref, so they would collide); give them distinct values. The error's
`= help:` line reads ``give the components a Payload field that tells them apart, for example the target page of a prev/next pair (`Page { page: 2 }` and `Page { page: 4 }`)``.

**Runtime backstop.** Holes, spreads, non-literal text, labels and options are a static
lower bound only: `noctis::ui::validate` (§16) checks the actual tree before sending.
`view!` is the unrelated image Template macro.

### Literal macros

| Macro | Yields | Rule | Runtime path (same function) |
| --- | --- | --- | --- |
| `label!("…")` | `Label` | CV2-08, ≤ 80 | `Label::try_new(text, &limits)`, `Label::try_from(&'static str / String)` (`validate_button_label`) |
| `placeholder!("…")` | `Placeholder` | CV2-09, ≤ 150 | `Placeholder::try_new`, `Placeholder::try_from` (`validate_select_placeholder`) |
| `content!("…")` | `Content` | MSG-01, ≤ 2000 | `Content::try_new`, `Content::try_from` (`message::validate_content`, path `content`) |
| `alt!("…")` | `AltText` | ATT-06, ≤ 1024 (empty allowed) | `AltText::try_new`, `AltText::try_from` (`validate_attachment_description`) |
| `rgb!(0x…)` | `Rgb` | CV2-13, `0x000000..=0xFFFFFF` | `Rgb::try_from(u32)` (`validate_accent_color`) |

- All are in the prelude (and at `noctis::label!` etc.), measure with `discord_len`
  against `Limits::default()`, and work in a `const`
  (`const SAVE: Label = label!("Save");`).
- Each takes exactly one literal: `label!(name)`, `label!(1)`, `label!(b"x")`,
  `label!()` fail with `[CV2-08] label! takes one string literal` (help: use
  `Label::try_new(text, &limits)`); `rgb!(-1)` / `rgb!(1.0)` with
  `[CV2-13] rgb! takes one non-negative integer literal`.
- `Label`, `Placeholder`, `Content` and `AltText` are newtypes over `Cow<'static, str>`
  (`Debug + Clone + Eq + Hash`) with `as_str()`, `AsRef<str>` and
  `From<_> for Cow<'static, str>`, so they go wherever a builder takes text
  (`.label(SAVE)`, `.placeholder(PICK)`, `LegacyMessage::new(content!("…"))`,
  `MediaItem::new(card).alt(alt!("Profile card"))`).
- `try_new` / `TryFrom` return `Err(UiError::Limits { violations })` (empty path, or
  `content` for MSG-01) and never truncate, even in Lenient mode; truncation is
  `validate`'s.
- Localized text is runtime text: pass it as is (`validate` checks it) or through
  `Label::try_new(s.common.save(), &limits)?`.

#### Checked button label: `Label`

- **Where:** any button label setter (`RouteButton::label`, `LinkButton::label`, and
  `.button(label)` in `components!`); it converts into `Cow<'static, str>`.
- **Limits:** ([catalog §9](https://github.com/ErdemGKSL/noctis-rs/blob/main/docs/discord-rules.md#9-components-v2-messages)) CV2-08, at most 80 characters by `discord_len`; never truncated.
- **Builder:** `label!("Save")` (compile time, `const`-friendly), `Label::try_new(text,
  &limits)` or `Label::try_from(text)` (runtime, against `Limits::default()`);
  `.as_str()`. `Placeholder` (CV2-09, `placeholder!`) and `Content` (MSG-01,
  `content!`) work the same way.

## 15. Rule functions (`noctis::rules::component`, `message`, `attachment`)

The `noctis-ui` builders already make most shapes unrepresentable. These functions check
trees and values Noctis cannot see at compile time (dynamic trees, raw components,
user-supplied labels, colours, attachments) with the same counts, paths and order that
`components!` and the runtime validator use. They only report: they never truncate (in
`LimitMode::Lenient`, `noctis::ui::validate` truncates display text, never structure;
§16). Every
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
| `Select<'a>` | `{ kind, placeholder, min_values: Option<usize>, max_values: Option<usize>, default_values: usize, string_defaults: Option<&[&str]>, disabled }`; `Select::new(kind)`, `.placeholder(..)`, `.min_values(n)`, `.max_values(n)`, `.default_values(count)` (auto-populated selects only), `.string_defaults(&["b"])` (a String Select's `default_values` list: each must name an option, CV2-09 at `…default_values[i]`, and the count comes from the options it names), `.disabled(bool)` |
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
| `validate_row_buttons(usize, &Limits)` | CV2-06 a count of 1–5 Action Row buttons (what `ActionRow::try_buttons` and `components!` call) |
| `validate_section_texts(usize, &Limits)` | CV2-07 a count of 1–3 Section texts (what `Section::try_new` calls) |
| `validate_gallery_items(usize, &Limits)` | CV2-10 a count of 1–10 gallery items (what `MediaGallery::try_new` calls) |
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
| `validate_v2_not_empty(components: usize, &Limits) -> Vec<Violation>` | MSG-09: a V2 message has ≥ 1 top-level component; path `components`, `AtLeast(1)`, `Count(0)` |
| `validate_legacy_not_empty(content: &str, rows: usize, &Limits) -> Vec<Violation>` | MSG-09: legacy `content` that is not empty or whitespace-only, or ≥ 1 Action Row; path `content`, `Describe("non-empty content or at least one Action Row")`, `Text(content)` |
| `MentionIds<'a>` | `None` (default), `All` (`parse`) or `Only(&[u64])`; `parse` plus ids cannot be built (MSG-03) |
| `AllowedMentions<'a>` | `#[non_exhaustive]` `{ users, roles: MentionIds, everyone, replied_user }`; `AllowedMentions::new()` = `Default` = pings nobody (MSG-05); `.users(..)`, `.roles(..)`, `.everyone(bool)`, `.replied_user(bool)` |
| `validate_mentions(&AllowedMentions, &Limits)` | MSG-04 ≤ 100 ids each, paths `allowed_mentions.users`, `allowed_mentions.roles` |
| `validate_nonce(&str, &Limits)` / `validate_sticker_ids(count, &Limits)` | MSG-06 nonce ≤ 25 (path `nonce`), ≤ 3 stickers (path `sticker_ids`) |
| `validate_webhook_name(&str, &Limits)` | WH-04 1–80 by `discord_len`, and no `clyde` or `discord` in it (ASCII case-insensitive, `Describe`, `Text(name)`), path `name` |
| `WebhookThreadDecl<'a>` | `#[non_exhaustive]` `None`, `Id` (an existing `thread_id`) or `Name(&str)` (a new post's `thread_name`) |
| `validate_webhook_thread(thread_only: bool, WebhookThreadDecl, &Limits) -> Vec<Violation>` | WH-05: `thread_only` (a forum or media channel) needs `Id` or `Name` (path `thread_id`, `Text("none")`); a `Name` is 1–100 (`Limits::webhook_thread_name_len`, path `thread_name`) and only in a thread-only channel |
| `validate_static_components(&[Component]) -> Vec<Violation>` | WH-02: no select and no non-Link button at any depth (twilight `Component` trees), paths as in §16 |

Noctis has no embed API in v1 (MSG-02), so there is nothing to check for embeds.

### Attachments (`noctis::rules::attachment`)

| Item | What it is |
| --- | --- |
| `AttachmentDecl<'a>` | `#[non_exhaustive]` `{ filename, size: usize, description: Option<&str> }`; `AttachmentDecl::new("card.png", bytes.len())`, `.description(alt)` |
| `validate_attachments(&[AttachmentDecl], json_len, file_limit: Option<usize>, &Limits) -> Result<(), Vec<Violation>>` | ATT-03 count (≤ 10, path `attachments`), per file ATT-01 size (`attachments[k].size`) and ATT-06 alt text ≤ 1024 (`attachments[k].description`), each with note ``file `name` ``, then ATT-02 request total (files + `json_len`, saturating, ≤ 25 MiB, path `request`) |
| `file_limit` | the interaction's `attachment_size_limit`; when `Some`, it replaces `Limits::attachment_bytes` (20 MiB) |
| `validate_file_size(size, file_limit, &Limits)` / `validate_attachment_description(&str, &Limits)` | single ATT-01 / ATT-06 checks, path `""` |
| `validate_attachment_ref(url, &[AttachmentDecl], &Limits) -> Vec<Violation>` | ATT-05: an `attachment://<name>` URL must name one of the files (exact, case-sensitive); other URLs pass |
| `validate_rendered_image_alt(Option<&str>, &Limits) -> Vec<Violation>` | ATT-06 (FR-30): a rendered image's alt text is present (`None`, `""` or whitespace fails), path `""`; `validate` calls it for every rendered Thumbnail/MediaItem |

## 16. Runtime validation: validate and cost

`noctis::ui::validate` checks a built message against `noctis::rules::Limits` before it
is sent; `noctis::ui::cost` measures one. The Responder runs validate before every send;
call it yourself to check a message early (tests, previews, budgets).

```rust
use noctis::rules::{LimitMode, Limits};
use noctis::ui::{CostReport, LimitWarning, UiError, V2Message, cost, validate};

let mut msg = V2Message::new(body).attachments(files);
match validate(&mut msg, &limits) {
    Ok(report) => assert!(report.components <= limits.components),
    Err(UiError::Limits { violations }) => {
        for v in &violations {
            eprintln!("{v}"); // [CV2-08] container[0].section[2].accessory.label: limit at most 80, actual 81
        }
    }
    Err(other) => eprintln!("{other}"),
}
let budget: CostReport = cost(&msg, &limits); // never changes or sends
```

| Item | What it is |
| --- | --- |
| `validate<M: Validate>(&mut M, &Limits) -> Result<CostReport, UiError>` | every check below; `&mut` only so Lenient mode can truncate |
| `cost<M: Validate>(&M, &Limits) -> CostReport` | component count, text characters and missing-alt warnings; no truncation, no logging, no mutation |
| `Validate` | sealed; implemented for `V2Message` and `LegacyMessage` only |
| `CostReport` | `#[non_exhaustive]` `{ components: usize, text_chars: usize, warnings: Vec<LimitWarning> }`; V2: CV2-03 count at every depth and CV2-04 Text Display sum; legacy: rows plus their items, and `discord_len(content)` |
| `LimitWarning::Truncated(Violation)` | Lenient mode truncated a field; the violation it avoided, with the length before (`Count`) |
| `LimitWarning::MissingAlt { path }` | a Thumbnail or gallery item has no alt text of its own (`None`, `""` or whitespace): a non-rendered image in both modes, a rendered image in Lenient mode (Strict: `[ATT-06]` violation), and every one from `cost` |
| `LimitWarning::path()` / `.rule()` | the path; `Some(rule)` for `Truncated`, `None` for `MissingAlt` |
| `UiError::Limits { violations: Vec<Violation> }` | the failure, never empty; `Display` joins each violation's `Display` with `\n` |
| `Budget::of(&V2Message, &Limits) -> Budget` | what the message leaves: `components_left()` = `limits.components - cost.components`, `text_left()` = `limits.text_total_len - cost.text_chars`, both saturating at 0; reads only, Strict and Lenient alike (FR-62) |
| `Budget::fits<C: ContainerChild + Clone>(&self, &C) -> usize` | copies of one Container child that fit: `min(components_left / c, text_left / t)`, an axis of cost 0 unbounded |
| `Budget::fits_prefix<'a, C, I: IntoIterator<Item = &'a C>>(&self, I) -> usize` | how many items fit in order; stops at the first miss, pulls at most `k + 1`. Budgets only (CV2-03, CV2-04): no per-field rules, no CID-02 |

Warnings display as `truncated to fit: <violation>` and `<path>: image has no alt text`.
Every warning is returned in the `CostReport` and logged by `validate` with
`tracing::warn!` (target `noctis::ui`, fields `rule` (empty for `MissingAlt`) and
`path`), also when the message then fails.

**What it checks.** The tree is lowered into `noctis::rules::component` descriptors and
checked with `validate_components` (V2) or `validate_legacy_components` (legacy), so
paths, counts and order match `components!` and §15 exactly. Then:

| Rule | Check | Path |
| --- | --- | --- |
| CID-02 | two components with the same Route and Payload (`Describe("a custom_id unique within the message")`, `Text(id)` for a resolved id or ``Text("route `<id>`: equal Payload")`` for an unresolved select, signed or by-Ref one, note `first used at <path>`, help ``give the components a Payload field that tells them apart, for example the target page of a prev/next pair (`Page { page: 2 }` and `Page { page: 4 }`)``); the Responder re-checks after resolving, so the verdict is the same before and after sending | `P.custom_id`, P the later button or select, e.g. `row[0].button[1].custom_id`, `section[0].accessory.custom_id` |
| ATT-05 | an `attachment://` URL (File, Thumbnail, gallery item) names no file of the message's `AttachmentSet` (`Describe("attachment://<name> of a file in this message")`, `Text(url)`); `attachment://` alone fails too; `https://…` and `ATTACHMENT://…` are not references | `file[0].url`, `section[0].accessory.url`, `gallery[0].item[0].url` |
| ATT-06 | an `Attachment::alt` over 1024 (note ``file `<name>` ``) | `attachments[k].description`, k in `AttachmentSet::iter()` (name) order |
| MSG-01 | legacy content over 2000 | `content` |
| MSG-09 | a V2 message with no top-level component (`AtLeast(1)`); a legacy message with empty or whitespace-only content and no row | `components` / `content` |
| MSG-04 | more than 100 `Users::Only` / `Roles::Only` ids | `allowed_mentions.users`, `allowed_mentions.roles` |

Violation order: the `noctis::rules::component` result in its own order, then CID-02
(pre-order), ATT-05 (pre-order), ATT-06 (set order), MSG-01 (legacy), MSG-09, MSG-04
(users, then roles). Nothing is dropped. CV2-05, CV2-13 and CV2-14 never fail for typed
trees: placement and colours are typed, and numeric ids are assigned in pre-order when
`Components` serializes, so duplicates cannot be built.

Paths (`noctis::rules::component`'s scheme): `kind[i]`, `parent.kind[j]`, `.accessory`,
`.item[k]`, `.options[k]`, `.label`, `.url`, `.description`, `.placeholder`,
`.min_values`, `.max_values`, `.default_values`, `.custom_id`; kinds `container`,
`section`, `text`, `thumbnail`, `gallery`, `file`, `separator`, `row`, `button`,
`select`. A Section's text k is `P.text[k]` (used only in Lenient notes).

**Strict and Lenient.** `LimitMode::Strict` (the default) never changes the message. In
`LimitMode::Lenient` validate first truncates over-long display text with `…` (via
`noctis::rules::truncate`, never splitting a surrogate pair), each truncation one
`LimitWarning::Truncated` in this order:

1. per field in pre-order: button labels of Route and Link buttons (CV2-08, `P.label`),
   select placeholders (CV2-09, `P.placeholder`), String Select option labels and
   descriptions (CV2-09, `P.options[k].label` / `.description`), gallery item alt
   (CV2-10, `P.item[k].description`), Thumbnail alt (CV2-11, `P.accessory.description`);
2. Text Display content to fit the CV2-04 total (one warning, path `components`, the
   total before as `Count`, one `truncated <path>` note per changed text);
3. `Attachment` alt text (ATT-06, note ``file `<name>` ``);
4. legacy content (MSG-01).

Everything else always fails in both modes: counts (CV2-03, CV2-06, CV2-07, CV2-10,
options), URLs, option values, `min_values` / `max_values`, ids, CID-02, ATT-05, MSG-04,
MSG-09. When a message is truncated and still fails, it stays truncated. A limit of 0
truncates to `""`.

CV2-04 in Lenient mode, with `n` Text Displays in pre-order and the limit `L`: text `j`
may keep `L − used − (n − j − 1)` units (one unit stays reserved for each later text).
The first text over its allowance is truncated to it and every later text to one unit
(`…`, or itself when ≤ 1). It always fits when `L ≥ n`; when `L < n` nothing changes and
CV2-04 fails. Example: `"a"×3990` then 20× `"bbbbbbbbbb"` → `"a"×3979 + "…"` then 20× `"…"`
(4000 in total).
