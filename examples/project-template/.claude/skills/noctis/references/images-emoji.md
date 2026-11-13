# Application emojis: icons inside text, buttons and selects

An application emoji is a small picture the bot owns and can place **inside a line of
text, on a button or on a select option**, in every server and DM, with no attachment.
It is the cheap inline companion to rendered images: status dots, check marks, item
icons, currency symbols, the pieces of a text progress bar.

Covers: what Discord guarantees (§1), the emoji methodology, the defaults to follow
unless the user says otherwise (§2), making the art (§3), drawing glyphs with the image
engine (§4), uploading (§5), using them from code (§6), emoji progress bars (§7),
animated emojis (§8), emoji or attachment (§9).

**Noctis 0.1 has no application-emoji API of its own.** Uploading and listing go
through the twilight client that `ctx.http()` returns (§5); using an emoji needs only
its name and id (§6).

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

Not stated in those docs, so treat as conventions: the characters allowed in a name
(use 2 to 32 of `[A-Za-z0-9_]`, the form Noctis itself parses, `[IMG-04]`), and the size
an emoji is displayed at (text height; much smaller than 128 px, client behaviour).

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
9. **Upload once, reference by name.** Keep the PNGs in the repository
   (`assets/emoji/<name>.png`), upload them to the application once per environment,
   and look ids up by name at start (§5, §6): the ids differ between a test bot and
   the production bot.

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

Upload those PNGs in the Developer Portal, or let the bot upload its own renders (§5).

## 5. Uploading

### 5.1 By hand

Developer Portal → the application → Emojis: upload each PNG and name it. Good for a
handful of icons and for artwork that does not come from code. Repeat for every
application (test bot, production bot).

### 5.2 From the bot

`ctx.http()` is the App's twilight client; it has the four application-emoji requests
(`get_application_emojis`, `add_application_emoji`, `update_application_emoji`,
`delete_application_emoji`). An owner-only Command that uploads what is missing:

```toml
# Cargo.toml: the data URI needs base64
base64 = "0.22"
```

```rust
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;

const MAX_EMOJI_BYTES: usize = 256 * 1024;      // Discord: 256 KiB per emoji

/// The set this bot ships: (name, glyph, tint).
const EMOJI_SET: &[(&str, Glyph, u32)] = &[
    ("check", Glyph::Check, 0x23_A55A),
    ("check_white", Glyph::Check, 0xFF_FFFF),
    ("st_online", Glyph::Dot, 0x23_A55A),
    ("st_offline", Glyph::Ring, 0x80_848E),
    ("bar_on_a", Glyph::BarStart, 0x58_65F2),
    ("bar_on_b", Glyph::BarMid, 0x58_65F2),
    ("bar_on_c", Glyph::BarEnd, 0x58_65F2),
    ("bar_off_a", Glyph::BarStart, 0x4E_5058),
    ("bar_off_b", Glyph::BarMid, 0x4E_5058),
    ("bar_off_c", Glyph::BarEnd, 0x4E_5058),
];

#[derive(SlashCommand, Debug)]
#[command("emoji sync", permissions = ADMINISTRATOR, contexts = [Guild], response = ephemeral, module = "emoji")]
pub struct EmojiSync;

impl EmojiSync {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let app = ctx.http().current_user_application().await?.model().await?.id;
        let existing = ctx.http().get_application_emojis(app).await?.model().await?.items;
        for &(name, glyph, tint) in EMOJI_SET {
            if existing.iter().any(|e| e.name == name) {
                continue;                                   // uploaded earlier: keep its id
            }
            let png = ctx.render(EmojiGlyph { glyph, tint }).await?;
            if png.bytes().len() > MAX_EMOJI_BYTES {
                return Err(HandlerError::user(keys::EMOJI_TOO_LARGE));
            }
            let image = format!("data:image/png;base64,{}", STANDARD.encode(png.bytes()));
            ctx.http().add_application_emoji(app, name, &image).await?;
        }
        ctx.reply_text(keys::EMOJI_SYNCED).await
    }
}
```

- Publish it as a guild-scoped Command of your own server (`guilds = [<id literal>]`
  in `#[command(..)]`, then no `contexts`), so it is not offered anywhere else.
- Each upload is one request under Discord's rate limits. A run that fails half way
  can simply be repeated: the loop skips what exists.
- A changed picture needs a new upload: delete the old emoji
  (`delete_application_emoji(app, id)`) or, better, upload under a new name and keep
  the old one for messages already sent. `update_application_emoji` only renames.
- Files that do not come from a Template (generated art in `assets/emoji/`) upload the
  same way from `include_bytes!` data instead of a render.
- After a sync, restart the bot or reload the table (§6) so the new ids are known.

## 6. Using them from code

Look the ids up by name once, at start, and keep them in App state:

```rust
use std::collections::BTreeMap;
use std::sync::OnceLock;

use noctis::id::marker::EmojiMarker;
use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;
use noctis::ui::{ButtonStyle, Emoji};

/// name → id of this application's emojis. Registered with `.state(AppEmojis::default())`.
#[derive(Debug, Default)]
pub struct AppEmojis(OnceLock<BTreeMap<String, Id<EmojiMarker>>>);

impl AppEmojis {
    fn id(&self, name: &str) -> Option<Id<EmojiMarker>> {
        self.0.get()?.get(name).copied()
    }

    /// For a button or a select option. `None` until the emoji exists.
    pub fn emoji(&self, name: &'static str) -> Option<Emoji> {
        self.id(name).map(|id| Emoji::custom(id, name, false))
    }

    /// The token for Text Display content. Empty until the emoji exists.
    pub fn inline(&self, name: &str) -> String {
        self.id(name).map(|id| format!("<:{name}:{id}>")).unwrap_or_default()
    }

    /// Methodology rule 5: white on a coloured button, coloured on the grey one.
    pub fn on(&self, style: ButtonStyle, name: &'static str, white: &'static str) -> Option<Emoji> {
        match style {
            ButtonStyle::Secondary => self.emoji(name),
            _ => self.emoji(white),      // Primary, Success, Danger
        }
    }
}

#[derive(Hook, Debug)]
#[hook(module = "emoji")]
pub struct LoadEmojis;

impl LoadEmojis {
    #[handler]
    async fn handle(self, ctx: HookCtx, _started: AppStarted) -> HandlerResult<()> {
        let Some(table) = ctx.state::<AppEmojis>() else { return Ok(()) };
        let app = ctx.http().current_user_application().await?.model().await?.id;
        let list = ctx.http().get_application_emojis(app).await?.model().await?;
        let _ = table.0.set(list.items.into_iter().map(|e| (e.name, e.id)).collect());
        Ok(())
    }
}
```

Handlers read the table with `ctx.state::<AppEmojis>()` and pass `&AppEmojis` to the
render function like `&Strings`.

**On buttons** (the style decides the variant):

```rust
pub fn confirm_row(order: OrderId, e: &AppEmojis) -> ActionRow {
    let mut confirm = ConfirmOrder { order }.build();          // defaults(style = Success, ..)
    if let Some(emoji) = e.on(ButtonStyle::Success, "check", "check_white") {
        confirm = confirm.emoji(emoji);                          // white: Success is coloured
    }
    let mut cancel = CancelOrder { order }.build();            // defaults(style = Secondary, ..)
    if let Some(emoji) = e.on(ButtonStyle::Secondary, "cross", "cross_white") {
        cancel = cancel.emoji(emoji);                            // coloured: Secondary is grey
    }
    ActionRow::buttons([Button::from(confirm), Button::from(cancel)])
}
```

When a button's style changes per render (Enable is `Success`, Disable is `Danger`),
pass the same `style` to `.style(..)` and to `e.on(..)` so the pair cannot drift apart.

**On select options** (coloured variant):

```rust
let mut option = SelectOption::new(s.shop.item_name(item), item.key());
if let Some(emoji) = e.emoji(item.icon()) {
    option = option.emoji(emoji);
}
```

**In text** (coloured variant):

```rust
TextDisplay::new(format!(
    "{} **{}** · {}",
    e.inline("st_online"),
    escape_markdown(&member.name),       // escape user text, never the token
    s.status.online(),
))
```

- **Never pass a token through `escape_markdown`**: it escapes the `_` in
  `check_white` and the emoji stops rendering. Escape the user text, then concatenate.
- **A token is text for the budget**: `<:st_online:1300000000000000001>` is 32
  characters of the 4000 (CV2-04). Short names matter in long lists.
- **Only the `emoji` field renders on a button or an option.** A token inside a label,
  a placeholder or a modal title is shown as plain characters (client behaviour).
- **A missing emoji degrades to nothing**: `emoji(..)` is `None`, `inline(..)` is
  empty. Keep labels and words complete without it (methodology rule 7).
- **A fixed id as a literal**: `#[button("shop.buy", defaults(style = Success, label =
  keys::SHOP_BUY, emoji = "<:cart_white:1300000000000000003>"))]` is checked at
  `cargo check`, but pins one application's id into the code: fine for a bot with a
  single application, wrong when a test bot and a production bot share the source.
- **Unicode emoji need no upload**: `Emoji::unicode("🔄")`. Use them for throwaway
  marks; use application emojis when the set must look like one family.

**Inside rendered images**: a token in a Template's *data* draws inline, fetched from
the Discord CDN like any custom emoji (`images-engine.md` §7 "Custom emoji"), so
`format!("{} {}", e.inline("coin"), price)` works as a `String` field of a card; and
`RemoteImage::custom_emoji(id, false)` in an `#[asset]` field draws one at any size.
The same icon set then appears in text, on buttons and in pictures.

## 7. Emoji progress bars

A bar made of emojis lives in the text itself: no attachment, no render per value, and
it sits on the same line as its numbers.

```rust
/// `filled` of `total` segments, e.g. 4 of 10.
pub fn emoji_bar(e: &AppEmojis, filled: usize, total: usize) -> String {
    (0..total)
        .map(|i| {
            let set = if i < filled { "on" } else { "off" };
            let piece = if i == 0 { "a" } else if i + 1 == total { "c" } else { "b" };
            e.inline(&format!("bar_{set}_{piece}"))
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
  fps = 15, format = Gif)` (the `image-gif` feature) and upload the bytes with
  `data:image/gif;base64,…`; flat colours, one-bit transparency, so use hard edges and
  no soft shadows (`images-cookbook.md` §10).
- Check the size before uploading: the limit is 256 KiB, far below the render budget's
  2 MiB default. Fewer frames or flatter shapes if it is over.
- Use them sparingly: a spinner on a "working" line, a pulse on a live status. A
  message full of moving glyphs is unreadable, and clients can be set not to animate
  emoji (client behaviour).
- Keep `animated` with the name in your table when you ship animated emojis (the
  emoji objects from `get_application_emojis` carry `animated: bool`), since the token
  and `Emoji::custom` both need it.

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
