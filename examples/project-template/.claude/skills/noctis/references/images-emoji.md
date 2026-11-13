# Application emojis: icons inside text, buttons and selects

An application emoji is a small picture the bot owns and can place **inside a line of
text, on a button or on a select option**, in every server and DM, with no attachment.
It is the cheap inline companion to rendered images: status dots, check marks, item
icons, currency symbols, the pieces of a text progress bar.

Covers: what Discord guarantees (§1), the emoji methodology, the defaults to follow
unless the user says otherwise (§2), making the art (§3), drawing glyphs with the image
engine (§4), declaring the set in code and uploading (§5), using them from code (§6),
emoji progress bars (§7), animated emojis (§8), emoji or attachment (§9).

**Declare the set in code** with `#[derive(AppEmojis)]` (§5.0): names, files, formats
and sizes are checked at `cargo check`. The App lists the application's emojis at start
and `ctx.emojis()` looks them up by the typed variant (§6). Publish uploads the missing
ones (§5.1): no upload code, no Developer Portal step.

## 1. What Discord guarantees

From Discord's Emoji resource (`developers/resources/emoji` in the API docs), section
"Application-Owned Emoji" and the application emoji endpoints:

- An application can own **up to 2000 emojis** that can **only be used by that app**.
- They are managed **with a bot token through the API, or in the app's settings in the
  Developer Portal**.
- The `USE_EXTERNAL_EMOJIS` permission is **not required** to use them.
- Emojis and animated emojis have a **maximum file size of 256 KiB**; a larger upload
  fails with 400 Bad Request.
- Create takes `name` and `image`, "the 128x128 emoji image", as image data (a data
  URI, `data:image/png;base64,…`).
- Emoji can be uploaded as JPEG, PNG, GIF, WebP and AVIF; clients are served WebP.
- Endpoints: List `GET /applications/{application.id}/emojis` (an object with the
  emojis under `items`), Get `GET …/emojis/{emoji.id}`, Create `POST …/emojis`,
  Modify `PATCH …/emojis/{emoji.id}` (the `name` only), Delete `DELETE
  …/emojis/{emoji.id}`.
- In message text a custom emoji is written `<:NAME:ID>`, an animated one
  `<a:NAME:ID>` (API Reference, "Message Formatting"). Buttons and select options take
  a partial emoji object (`id`, `name`, `animated`).

Noctis enforces these as `[EMO-01]` (2000 per application), `[EMO-02]` (256 KiB =
262,144 bytes), `[EMO-03]` (a name is 2 to 32 of `[A-Za-z0-9_]`, the form Noctis also
parses in images, `[IMG-04]`, and unique per application) and `[EMO-04]` (Noctis
uploads PNG, JPEG or GIF only: "Image Data" names only those three; a GIF is
animated). 128×128 is a recommendation, not a check. The size an emoji is displayed at
is client behaviour (text height; much smaller than 128 px).

## 2. Emoji methodology (defaults)

Follow this checklist unless the user of the bot project says otherwise.

1. **One icon set, one style.** Draw every emoji in a **vector style**: flat, crisp
   geometric shapes, even stroke weight, no photos, no gradients, no fine detail, no
   text. They are exported and uploaded as PNG (emojis are raster images); "vector" is
   how they are drawn, not the file format.
2. **Legible when tiny.** An emoji is shown at about the height of a line of text. One
   bold shape that fills most of the 128 × 128 canvas; check it scaled down to about
   20 px before uploading.
3. **Transparent background**, always. No tile, no circle behind the glyph unless the
   circle is the glyph.
4. **Two variants of every icon**: a **white** one and a **coloured** one, named
   `<icon>_white` and `<icon>` (`check_white` / `check`, `trash_white` / `trash`).
5. **White on coloured buttons.** An emoji on a `Primary` (blurple), `Success` (green)
   or `Danger` (red) button is the white variant: a coloured emoji on a coloured button
   looks bad and can vanish (a green check on a green button). The coloured variant is
   for the grey `Secondary` button, for Link buttons, for select options and for plain
   text.

   | Where | Variant |
   | --- | --- |
   | `ButtonStyle::Primary`, `ButtonStyle::Success`, `ButtonStyle::Danger` | white (`check_white`) |
   | `ButtonStyle::Secondary`, `LinkButton` | coloured (`check`) |
   | select option, Text Display content | coloured (`check`) |

6. **Names are an API.** Lowercase `snake_case`, 2 to 32 characters, stable once
   released (messages already sent carry the name and the id). Prefix families:
   `ic_` for interface icons, `st_` for states, `bar_` for bar pieces.
7. **Meaning never rests on the emoji alone.** A button keeps its label; a status line
   keeps its word ("Online"). An emoji can fail to load, and its name is all
   assistive technology has to read.
8. **Coloured variants use one palette**: the bot's theme colours (the `--color-*`
   tokens of `noctis-theme.css`), each with enough contrast on both dark and light
   clients; avoid pure white, near-black and pale tints for the coloured variant.
9. **Declare once, reference by name.** Keep the PNGs in the repository
   (`assets/emoji/<name>.png`), let Publish upload them to each application,
   and look them up with `ctx.emojis()` (§6), never by a hard-coded id: the ids differ
   between a test bot and the production bot.

## 3. Making the art

Choose by what the agent can do, in this order:

1. **An image-generation skill or tool, if the session has one**: use it to draw the
   set, asking for the style of §2 (flat vector icon, single centred shape, transparent
   background, 128 × 128 or larger and square, no text), once for the coloured variant
   and once for the white one, with the same shape. Check the result at 20 px.
2. **Otherwise draw them in code**: with the Noctis image engine (§4), which is exact
   for geometric glyphs and makes the white variant by changing one tint field; or as
   SVG written by hand and exported to PNG with whatever converter the project already
   has.

Either way the output is a set of square PNG files with transparency, each under
256 KiB (a flat 128 × 128 PNG is about 1 KB).

## 4. Drawing glyphs with the image engine

One Template draws the whole family; `tint` makes the variant.

```rust
use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Dot,        // status
    Ring,       // "empty" status
    Check,
    Plus,
    Chevron,    // next / open
    BarStart,   // the three pieces of a text progress bar (§7)
    BarMid,
    BarEnd,
}

#[derive(Template, Serialize, Deserialize, Debug, Clone, Copy)]
#[template(id = "emoji.glyph", version = 1, size = (128, 128), format = Png, module = "emoji")]
pub struct EmojiGlyph {
    pub glyph: Glyph,
    pub tint: u32,     // 0xFFFFFF for the white variant
}

impl TemplateView for EmojiGlyph {
    fn view(&self, _frame: Frame) -> Node {
        let fill = Style::new().bg_rgb(self.tint);
        let stroke = Style::new().border_color_rgb(self.tint);
        match self.glyph {
            Glyph::Dot => view! {
                <div tw="flex items-center justify-center w-full h-full">
                    <div tw="w-20 h-20 rounded-full" style={fill} />
                </div>
            },
            Glyph::Ring => view! {
                <div tw="flex items-center justify-center w-full h-full">
                    <div tw="w-24 h-24 rounded-full border-[16px]" style={stroke} />
                </div>
            },
            // two borders of a box, turned 45°: a check mark
            Glyph::Check => view! {
                <div tw="flex items-center justify-center w-full h-full pb-4">
                    <div tw="w-12 h-24 border-r-[18px] border-b-[18px] rounded-sm"
                         style={stroke.rotate_deg(45.0)} />
                </div>
            },
            Glyph::Plus => view! {
                <div tw="relative flex items-center justify-center w-full h-full">
                    <div tw="absolute w-24 h-5 rounded-full" style={fill} />
                    <div tw="absolute w-5 h-24 rounded-full" style={fill} />
                </div>
            },
            Glyph::Chevron => view! {
                <div tw="flex items-center justify-center w-full h-full pr-6">
                    <div tw="w-16 h-16 border-t-[18px] border-r-[18px] rounded-sm"
                         style={stroke.rotate_deg(45.0)} />
                </div>
            },
            // bar pieces touch both side edges, so neighbours join into one bar
            Glyph::BarStart => view! {
                <div tw="flex items-center w-full h-full">
                    <div tw="w-full h-16 rounded-l-full" style={fill} />
                </div>
            },
            Glyph::BarMid => view! {
                <div tw="flex items-center w-full h-full">
                    <div tw="w-full h-16" style={fill} />
                </div>
            },
            Glyph::BarEnd => view! {
                <div tw="flex items-center w-full h-full">
                    <div tw="w-full h-16 rounded-r-full" style={fill} />
                </div>
            },
        }
    }
}
```

- The root has no background, so the PNG is transparent.
- Shapes come from boxes: circles (`rounded-full`), bars, borders, rotations. A cross
  is a plus rotated 45°; an arrow is a chevron with a bar; a pause sign is two bars; a
  stop sign is a rounded square. Glyphs that need curves or real artwork (a trash can,
  a gear, a coin) are jobs for an image generator or a hand-made PNG, drawn as
  `StaticImage`s if they are also needed inside cards.
- Strokes of 16 to 20 px on the 128 px canvas stay visible at text size.

**Getting the files.** A pixel snapshot test writes each render as
`tests/snapshots/<name>.png` on its first run, which is exactly an export:

```rust
#[tokio::test(flavor = "multi_thread")]
async fn emoji_glyphs() -> anyhow::Result<()> {
    let app = TestApp::build(configure(&mut App::builder())).await?;
    for (name, glyph, tint) in [
        ("check", Glyph::Check, 0x23_A55A),
        ("check_white", Glyph::Check, 0xFF_FFFF),
        ("st_online", Glyph::Dot, 0x23_A55A),
        ("st_offline", Glyph::Ring, 0x80_848E),
    ] {
        assert_render_snapshot!(app.renderer(), name, EmojiGlyph { glyph, tint });
    }
    app.stop().await?;
    Ok(())
}
```

Commit those PNGs and declare them (§5.0), or declare the variant `#[emoji(render)]` and
let Publish render and upload it (§5.1).

## 5. Declaring and uploading

### 5.0 Declaring the set (`#[derive(AppEmojis)]`)

The set is a fieldless enum, one variant per icon, registered on the App builder:

```rust
use noctis::prelude::*;

#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
#[emojis(dir = "assets/emoji")]            // the default; relative to the crate root
pub enum Icons {
    #[emoji(white)]
    Check,                                  // check (check.png) and check_white (check_white.png)
    #[emoji(white)]
    Cross,
    StOnline,                               // st_online (st_online.png)
    #[emoji(name = "coin", file = "money/coin.gif")]
    Coin,                                   // a GIF is an animated emoji
}

// in configure(builder):
builder.emojis::<Icons>();
```

- **Names**: the variant in `snake_case` (`StOnline` → `st_online`);
  `#[emoji(name = "..")]` overrides it. `#[emoji(white)]` also declares
  `<name>_white` (rule 4 of §2). Names are an API: renaming a variant renames the emoji.
- **Files**: `<dir>/<name>.png`, `.gif` or `.jpg` (exactly one may exist);
  `#[emoji(file = "..")]` is relative to `dir`, and its white file is
  `<stem>_white.<ext>` beside it. Files are embedded with `include_bytes!`, so the
  binary carries them and an edit rebuilds the crate.
- **Checked at `cargo check`**, each error on the variant: a bad name or a name the set
  declares twice (`<name>_white` included) is `[EMO-03]`; a missing file, or one that
  is not PNG, JPEG or GIF by its first bytes (WebP and AVIF are not uploaded), is
  `[EMO-04]`; a file over 256 KiB is `[EMO-02]`.
- **Checked at `build()`**: a name two registered sets declare is
  `BuildViolation::DuplicateEmoji` (`[EMO-03]`); more than 2000 names, `_white`
  included, is `BuildViolation::TooManyEmojis` (`[EMO-01]`). Registering one set twice
  is harmless; an App without a set makes no emoji request.
- **Drawn icons**: mark a variant `#[emoji(render)]` (no file is read) and give the set
  one art function, `#[emojis(art = glyph_art)]` with
  `fn glyph_art(icon: Icons, tone: EmojiTone) -> EmojiArt`, returning
  `EmojiArt::template(EmojiGlyph { .. })` (§4; `EmojiTone::{Colour, White}` picks the
  tint). `render` without `art`, or `art` with no `render` variant, does not compile.
- **Reading the set**: `Icons::Check.declaration()` gives `name()`, `white_name()`,
  `image()` (`EmojiImage::File { bytes, format }` or `EmojiImage::Render(f)`), and
  `Icons::DECLARATIONS` lists them all; `app.emoji_declarations()` lists every name the
  App declares. Publish uploads them (§5.1).

The items, all in `noctis::emoji` (`AppEmojis`, `EmojiArt` and `EmojiTone` are also in
the prelude):

| Item | What it is |
| --- | --- |
| `AppEmojis` | the derive, and the trait it implements: `DECLARATIONS` (one `EmojiDecl` per variant, in order) and `declaration(self)` |
| `EmojiDecl` | one variant: `name()`, `image()`, `white_name()`, `white_image()`, `name_in(EmojiTone)` (`None` for `White` without a white variant) |
| `EmojiImage` | `File { bytes, format }` (embedded, already checked) or `Render(fn(EmojiTone) -> EmojiArt)`; `animated()` is `Some(true)` for a GIF, `None` for a drawn one |
| `EmojiFormat` | `Png`, `Jpeg`, `Gif`: `sniff(bytes)` by magic bytes, `is_animated()`, `mime()`, and `EXTENSIONS` (`png`, `gif`, `jpg`, the order files are looked up in) |
| `EmojiTone` | `Colour` or `White` |
| `EmojiArt` | `EmojiArt::template(t)` for a drawn icon; `template_id()` |
| `EmojiDeclarations` | what `app.emoji_declarations()` returns: `len()`, `is_empty()`, `all()` (a slice, in fold order), `get(name)` |
| `DeclaredEmoji` | one declared name of the App: `name()`, `image()`, `tone()`, `set()` (the set's type name) |

`BuildViolation::DuplicateEmoji { name, first_set, second_set }` and
`BuildViolation::TooManyEmojis { count, limit }` are the `build()` errors;
`BuildViolation::InvalidEmoji { set, violation }` appears only for a hand-written
`AppEmojis` impl (the derive rejects a bad name at `cargo check`).

To check names or files built at run time (an admin uploading a picture), use
`noctis::rules::emoji`: `validate_emoji_name(name)` (EMO-03, `NAME_MIN` 2 to
`NAME_MAX` 32), `validate_emoji_image(name, bytes, &limits)` (EMO-04 then EMO-02,
returning the `EmojiFormat`) and `validate_emoji_set(names, &limits)` (EMO-03
uniqueness and EMO-01); each returns the `Violation` with its rule id.

### 5.1 Publish uploads them

Every Publish uploads the declared set: `cargo run --release -- --publish` (or
`--publish-guild [<id>]`, `NOCTIS_PUBLISHER=true` on one process, `app.publish(..)`).
After the Commands it lists the application's emojis and creates every declared name
that is missing, one request at a time through the App's HTTP client and rate limiter.
There is no upload Command to write and no Developer Portal step, and a new
application (test bot, production bot) gets the whole set from its first Publish.

- **Only the publishing process writes.** Every other process only lists (§6); none
  creates, renames or deletes an emoji. `--publish-clear` leaves emojis alone.
- **Files** upload as they are (PNG, JPEG or GIF; a GIF is animated). **Drawn icons**
  (`#[emoji(render)]`) are rendered through the App's Renderer from the set's `art`
  function as PNG, or GIF when the Template animates, capped at 256 KiB
  (`RenderOptions::max_bytes(262_144)`, so it degrades fps, size, quality to fit).
  An animation that fits only as a still frame is an error, not an upload. Without
  a Renderer (`.theme(..)` / `.image(..)`) such a set fails the Publish with
  `PublishError::EmojiNeedsRenderer { name }` before any request. The art Templates
  are registered with the Renderer at `build()`; no `Module::template` is needed.
- **Never touched**: emojis the code does not declare (uploaded by hand) stay.
- **Changed art** is detected by a blake3 digest of the uploaded bytes, kept in the
  App's `Kv` (Discord returns no hash and re-encodes the image). By default a changed
  emoji is reported as `changed` and kept. An emoji with no stored digest (uploaded by
  hand, or the store was empty) is adopted as unchanged, so with the in-memory default
  store a change is only seen while one process publishes repeatedly; give the App a
  durable `CacheStore` adapter for it to survive restarts.
- **Opt-ins**, on the builder:

  ```rust
  use noctis::emoji::EmojiSync;

  builder.emoji_sync(
      EmojiSync::new()
          .replace_changed(true)     // delete and re-create a changed emoji
          .delete_undeclared(true),  // delete emojis no set declares
  );
  ```

  `EmojiSync` is `Copy` and `Default` (= `new()`, both off); `replaces_changed()` and
  `deletes_undeclared()` read it back. Replacing gives the emoji a **new id**: messages already sent keep the old
  `<:name:id>` token, which then shows as plain `:name:` text. Prefer a new name
  (a new variant) for a redesigned icon when old messages matter.
- **Checks before the first write**: the creates would take the application over 2000
  emojis → `PublishError::TooManyEmojis { count, max }` (`[EMO-01]`). A 400 on a
  create is `PublishError::EmojiRejected { name, status, code }`, naming `[EMO-02]`
  (over 256 KiB) and `[EMO-04]` (not a valid PNG, JPEG or GIF) as the likely rules.
- **A failed sync is safe to repeat**: the commands are already published, and the next
  Publish's list skips the names created before the failure.
- **`--dry-run`** sends only the list and prints the plan.
- **The report**: `report.emojis()` is `Option<&EmojiReport>` (`None` without a set
  and for a clear) with `created()`, `unchanged()`, `changed()`, `replaced()`,
  `deleted()` (names, `&[String]`), `wrote()` and `has_writes()`; the CLI prints it
  under `== emojis ==`, and `PublishCompleted` hooks see it.
- After its own Publish the process re-lists, so `ctx.emojis()` knows the new ids at
  once; other processes re-list on their own within 60 s while a name is missing (§6).

```text
== emojis ==
+ create emoji `check_white`
~ changed emoji `logo` (kept: EmojiSync::replace_changed is off)
3 emoji(s) unchanged
written
```

### 5.2 By hand

Developer Portal → the application → Emojis: upload a PNG and name it. Use it only for
artwork that is not in code; look such an emoji up by its id or a `"<:name:id>"`
literal, because `ctx.emojis()` knows declared names only.

## 6. Using them from code

The App lists the application's emojis once at start (`GET
/applications/{id}/emojis`, through its one HTTP client) and builds the name → id
table before any handler runs. `ctx.emojis()` returns it in every context
(`CommandCtx`, `ComponentCtx`, `ModalSubmitCtx`, `ExpiredCtx`, `AutocompleteCtx`,
`EventCtx`, `HookCtx`), and `app.emojis()` outside one. Pass it to the render function
like `&Strings`:

```rust
use noctis::emoji::Emojis;
use noctis::prelude::*;
use noctis::ui::{ActionRow, Button, ButtonStyle, SelectOption, TextDisplay};

use crate::emoji::Icons;

// in a handler:
ctx.respond(render_order(order, &s, ctx.emojis())).await
```

| Call | Returns | Use it for |
| --- | --- | --- |
| `emojis.get(Icons::Check)` | `Option<Emoji>`, the coloured variant | select options, `LinkButton`, `Secondary` buttons |
| `emojis.white(Icons::Check)` | `Option<Emoji>`, `check_white` | rarely: `on` picks it for you |
| `emojis.on(style, Icons::Check)` | `Option<Emoji>`: white on `Primary`, `Success` and `Danger` when the icon declares `#[emoji(white)]` (coloured while `check_white` is missing), else coloured; coloured on `Secondary` | every `RouteButton` |
| `emojis.text(Icons::Check)` | `EmojiText`, a `Display` token: `<:check:ID>`, `<a:spin:ID>` when animated, **nothing** while missing; `is_missing()` | Text Display content, Template data |
| `emojis.id("check")` | `Option<Id<EmojiMarker>>` by declared name | `RemoteImage::custom_emoji(id, ..)` |
| `emojis.missing()` | the declared names the last list did not find | a status or admin screen |
| `emojis.is_loaded()` | whether a list has succeeded | the same |
| `emojis.reload().await` | `Result<(), EmojiListError>`: list now | after an upload by hand (§5.2); a Publish re-lists itself |
| `emojis.declarations()` | the `EmojiDeclarations` | listing the set |

No lookup does I/O or allocates more than the value it returns; `Emojis` is a cheap
`Clone` (`ctx.emojis().clone()` into a spawned task). Every App has its own table.

**On buttons** (the style decides the variant): `.emoji(..)` on `RouteButton`,
`LinkButton` and `SelectOption` takes the `Option<Emoji>` directly, and `None` leaves
the component without an emoji (also replacing a `defaults(emoji = ..)`).

```rust
pub fn confirm_row(order: OrderId, e: &Emojis) -> ActionRow {
    let confirm = ConfirmOrder { order }.build()               // defaults(style = Success, ..)
        .emoji(e.on(ButtonStyle::Success, Icons::Check));       // check_white
    let cancel = CancelOrder { order }.build()                 // defaults(style = Secondary, ..)
        .emoji(e.on(ButtonStyle::Secondary, Icons::Cross));    // cross
    ActionRow::buttons([Button::from(confirm), Button::from(cancel)])
}
```

When a button's style changes per render (Enable is `Success`, Disable is `Danger`),
compute the `style` once and pass it to `.style(style)` and to `e.on(style, ..)` so the
pair cannot drift apart (the Project Template's dashboard Modules tab does this).

**On select options** (coloured variant):

```rust
SelectOption::new(s.shop.item_name(item), item.key())
    .emoji(e.get(item.icon()))          // item.icon() -> Icons
    .default(item == current)
```

**In text** (coloured variant):

```rust
TextDisplay::new(format!(
    "{} **{}** · {}",
    e.text(Icons::StOnline),
    escape_markdown(&member.name),       // escape user text, never the token
    s.status.online(),
))
```

What happens when an emoji is missing (not uploaded yet, or the list failed):

- **Nothing fails.** The lookups return `None` and the token writes nothing, so keep
  labels and words complete without the emoji (methodology rule 7). Watch for a
  leading space when the token is empty (`format!("{} x", ..).trim_start()`).
- **Start-up logs it** in one `warn` (target `noctis::emoji`) naming the missing names
  and saying to run Publish. A failed list also only warns: the bot starts, the table
  stays empty, and the list is retried with backoff (1 s doubling to 60 s).
- **It heals on its own.** While a declared name is missing, the table is re-listed
  every 60 s (`AppBuilder::emoji_reload_interval(Duration)`; `Duration::ZERO` turns it
  off), and after every Publish of this process; a Cluster process picks up the
  publisher's upload without a restart.

Other rules:

- **Never pass a token through `escape_markdown`**: it escapes the `_` in
  `check_white` and the emoji stops rendering. Escape the user text, then concatenate.
- **A token is text for the budget**: `<:st_online:1300000000000000001>` is 32
  characters of the 4000 (CV2-04). Short names matter in long lists.
- **Only the `emoji` field renders on a button or an option.** A token inside a label,
  a placeholder or a modal title is shown as plain characters (client behaviour).
- **A fixed id as a literal**: `#[button("shop.buy", defaults(style = Success, label =
  keys::SHOP_BUY, emoji = "<:cart_white:1300000000000000003>"))]` is checked at
  `cargo check`, but pins one application's id into the code: fine for a bot with a
  single application, wrong when a test bot and a production bot share the source.
  Names in `defaults(emoji = ..)` are not supported (defaults are compile-time data);
  set the emoji per render with `.emoji(e.on(..))`.
- **Unicode emoji need no upload**: `Emoji::unicode("🔄")`. Use them for throwaway
  marks; use application emojis when the set must look like one family.

**In tests**, `TestApp` resolves every declared emoji to a stable fake id without a
request (`noctis::testkit::fake_emoji_id("check")`), `app.app_emojis(["check"])` keeps
only the names given (the degraded path), and `Response::button_by_emoji("check")`
finds a button by the declared name, its `check_white` variant included
(`testing.md`).

**Inside rendered images**: a token in a Template's *data* draws inline, fetched from
the Discord CDN like any custom emoji (`images-engine.md` §7 "Custom emoji"), so
`format!("{} {}", e.text(Icons::Coin), price)` works as a `String` field of a card; and
`RemoteImage::custom_emoji(id, false)` in an `#[asset]` field (with `e.id("coin")`)
draws one at any size. The same icon set then appears in text, on buttons and in
pictures.

## 7. Emoji progress bars

A bar made of emojis lives in the text itself: no attachment, no render per value, and
it sits on the same line as its numbers. The six pieces are variants of the set:

```rust
#[derive(AppEmojis, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bar {
    BarOnA, BarOnB, BarOnC,       // bar_on_a.png …: lit start, middle, end
    BarOffA, BarOffB, BarOffC,    // unlit
}

/// `filled` of `total` segments, e.g. 4 of 10.
pub fn emoji_bar(e: &Emojis, filled: usize, total: usize) -> String {
    (0..total)
        .map(|i| {
            let piece = match (i < filled, i == 0, i + 1 == total) {
                (true, true, _) => Bar::BarOnA,
                (true, _, true) => Bar::BarOnC,
                (true, _, _) => Bar::BarOnB,
                (false, true, _) => Bar::BarOffA,
                (false, _, true) => Bar::BarOffC,
                (false, _, _) => Bar::BarOffB,
            };
            e.text(piece).to_string()
        })
        .collect()
}

TextDisplay::new(format!(
    "**{}**\n{} {}",
    s.rank.level(level),
    emoji_bar(e, (xp * 10 / next.max(1)) as usize, 10),
    s.rank.xp_of(xp, next),
))
```

- Register the second set too (`builder.emojis::<Icons>().emojis::<Bar>()`); a name
  two sets declare fails `build()` with `[EMO-03]`.
- Six emojis make every state: start, middle and end, each lit and unlit. Pieces touch
  the left and right canvas edges (§4). Whether neighbouring emojis touch on screen is
  client behaviour: the pieces look intended both as one joined bar and as segments.
- **Cost in characters**: a token with a 9-character name and a 19-digit id is 32
  characters; a 10-segment bar is about 320 of the 4000, so a screen holds a few bars,
  not twenty. For many bars draw one stat sheet (`images-cookbook.md` §2.3).
- **Resolution is the segment count.** For a smooth, wide or animated bar, render a
  strip into a Media Gallery (`images.md` §5); for a compact bar inside a Section or
  next to text, use emojis.
- The size emojis are drawn at is client behaviour: look at the bar next to its
  numbers in a real client (desktop and phone) before settling on the segment count.

The same idea covers hearts (`heart`, `heart_empty`), stars, dice faces, card suits,
medal ranks in a text leaderboard, and status dots in member lists.

## 8. Animated emojis

An animated emoji is uploaded as an animated image and written `<a:name:id>`
(`Emoji::custom(id, name, true)`, and `format!("<a:{name}:{id}>")` in text).

- Draw it as an animated Template at 128 × 128 with `animation(duration_ms = 1000,
  fps = 15)` (the `image-gif` feature) and declare it `#[emoji(render)]`: Publish
  renders it as GIF within 256 KiB (§5.1); flat colours, one-bit transparency, so use
  hard edges and no soft shadows (`images-cookbook.md` §10).
- The limit is 256 KiB, far below the render budget's 2 MiB default; an animation
  that fits only as a still frame fails the Publish (`PublishError::EmojiArt`). Fewer
  frames or flatter shapes if it does.
- Use them sparingly: a spinner on a "working" line, a pulse on a live status. A
  message full of moving glyphs is unreadable, and clients can be set not to animate
  emoji (client behaviour).
- `ctx.emojis()` keeps `animated` from the list, so `get(..)` and `text(..)` of a GIF
  emoji are already animated (`<a:name:id>`); declare it as a `.gif` file (§5.0).

## 9. Emoji or attachment?

| | Application emoji | Rendered attachment |
| --- | --- | --- |
| Lives in | text, buttons, select options | Media Gallery, Thumbnail |
| Size on screen | text height | up to the message width |
| Per message | counts characters (CV2-04) | counts files, at most 10 (ATT-03) |
| Varies with data | no: a fixed set, uploaded ahead (2000 per application) | yes: any value, any text |
| Cost per use | none (an id in the text) | a render (often cached) and an upload |
| Text inside it | no | yes |
| Changes on a click | swap to another emoji of the set | re-render |

Use both on one screen: the banner and the stat sheet are images, the state marks in
the text and the icons on the buttons are emojis.
