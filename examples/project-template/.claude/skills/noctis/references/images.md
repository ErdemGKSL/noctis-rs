# Images: the drawing engine in a Components V2 UI

Noctis draws images from Rust: a **Template** is a struct (the data) with a `view`
(Tailwind-styled nodes), rendered off the async runtime into a PNG, JPEG, WebP, GIF or
APNG and attached to a message. This file is the entry: the mental model, the surfaces
an image can land on, the sizes that work, and a complete first example. Then read only
the topic file the task needs:

| File | Read when you… |
| --- | --- |
| `images-cookbook.md` | draw something: progress bars (still, animated, segmented), stat sheets, ring and 270° gauges (`Arc`), pie and donut charts (`Pie`) and dials, badges and status avatars, leaderboards, column, line and area charts (`Line`, `Area`), sparklines, heatmaps, banners and dividers, game boards, step indicators, countdowns, loading states, avatar stacks |
| `images-ui.md` | put images into a message: which surface, screen layouts, re-rendering on a click, paginated image pages, animation in a UI, attachment and size limits, performance, caching, when not to use an image, troubleshooting |
| `images-emoji.md` | need icons inside text, buttons or select options: application emojis, drawing and uploading them, the emoji methodology (white on coloured buttons), emoji progress bars |
| `images-engine.md` | need the exact rule: every `#[template(..)]` key, `view!` and `tw!` syntax, the class grammar, `Style`, the Theme file, fonts, static and remote images, emoji in images, render errors, pool, caps, size budget, cache, pixel snapshots |

Doctrine: UD-2 (images are read-only visuals; anything the user acts on is a component).

## 1. Mental model

```text
data (struct) ──view()──▶ Node tree ──Renderer (render pool)──▶ bytes ──▶ Attachment
Attachment ──AttachmentSet::add──▶ AttachmentRef ──▶ MediaItem | Thumbnail ──▶ V2Message
```

- **One Template = one picture kind.** Its fields are everything that varies (labels,
  numbers, colours, avatars); its `view` is a pure function of them. The same data
  always gives the same bytes and the same file name.
- **Rendering is awaited in the handler** (`ctx.render(t).await?`), never in a render
  function: the handler loads data, renders the images, then calls the pure
  `render_x(..)` that places them. The auto-defer covers a slow render.
- **An image is not a component tree.** It cannot be clicked, selected, translated by
  the client, read by a screen reader or searched. Whatever it shows that matters is
  also in its alt text; whatever the user acts on is a button or a select next to it.
- **A message is rebuilt as a whole.** A click re-renders the image with new data and
  `ctx.update(..)` swaps it; an image whose data did not change is kept by Discord
  without a new upload (`images-ui.md` §4).

## 2. Where an image can land

| Surface | Noctis | Shape and role | Limits |
| --- | --- | --- | --- |
| Media Gallery, 1 item | `MediaGallery::new([MediaItem::new(r).alt(..)])` | the **canvas**: one image across the message (or Container) width. Banners, cards, charts, boards, and thin strips such as a progress bar or a divider | 1 component for any number of items (CV2-03); alt ≤ 1024 (CV2-10) |
| Media Gallery, 2 to 10 items | `MediaGallery::new([a, b])`, `MediaGallery::try_new(items, &limits)` | a **grid** the client arranges: comparisons, galleries, several small panels | 1 to 10 items (CV2-10) |
| Thumbnail | `Section::new([texts], Thumbnail::new(r).alt(..))` | a **small picture beside 1 to 3 Text Displays**: avatar, badge, icon, gauge, sparkline. Stills and animations, no video (CV2-11) | only as a Section accessory (CV2-05, CV2-07); Section + texts + Thumbnail = 3 to 5 components |
| File | `File::new(r)` | a **download row** (name and size), not a picture: exports, logs, the full-size original | `attachment://` only (CV2-12) |
| Custom emoji | `<:name:id>` in Text Display content, `Emoji::custom(..)` on buttons and select options | a **glyph at text size**, uploaded once to the application, no attachment at all (`images-emoji.md`) | 2000 per application, 256 KiB each (Discord's Emoji resource) |
| Container accent | `Container::new().accent(rgb!(0x5865_F2))` | not an image, but the colour bar that ties a rendered card to its Container: use the Template's own tint | 0x000000 to 0xFFFFFF (CV2-13) |

All of them live in a V2 message with at most 40 components (CV2-03), 4000 Text Display
characters (CV2-04) and 10 attachments (ATT-03). An image costs no text characters and a
whole gallery costs one component, which is why dense read-only data belongs in one.

**Display size is client behaviour, not a documented rule.** Discord's component
reference gives no pixel widths. What clients have been observed to do: a single gallery
item is scaled to the available width and keeps its aspect ratio (so a thin strip stays
a thin strip), several items are laid out as a cropped grid, and a Thumbnail is a small
rounded square at the right of its Section. Design for scaling, never for exact pixels:

- Keep the text in an image large: the image is scaled down on phones. At a 900 px wide
  canvas, body text below `text-xl` (20 px) becomes hard to read; prefer `text-2xl` and up.
- Keep the important content away from the edges of gallery items in a grid (they can
  be cropped) and inside the centre circle of a Thumbnail.
- Never put text that carries meaning into a Thumbnail: it is small everywhere.

## 3. Canvas sizes that work

`size = (w, h)` is the encoded pixel size. Larger costs bytes and render time without
looking sharper once the client scales it down. These are conventions that fit the caps
(4 MP per job, 1 MP per animation frame; `images-engine.md` §8), not Discord rules:

| Use | Size | Ratio | Notes |
| --- | --- | --- | --- |
| Header banner | 1200 × 300 | 4:1 | `Jpeg { quality: 88 }` for gradients and photos |
| Profile or rank card | 934 × 282 | ≈ 10:3 | the template's `ProfileCard` |
| Leaderboard, table | 900 × 600 | 3:2 | at most about 8 rows; paginate the rest |
| Chart | 900 × 420 | ≈ 2:1 | |
| Progress strip | 800 × 40 | 20:1 | bar only; the numbers go in a Text Display |
| Labelled bar | 800 × 96 | ≈ 8:1 | |
| Step indicator | 900 × 120 | 15:2 | |
| Divider | 1200 × 12 | 100:1 | a drawn rule instead of a Separator |
| Square board | 512 × 512 | 1:1 | game boards, QR-like grids |
| Thumbnail (badge, gauge, avatar) | 256 × 256 | 1:1 | |
| Emoji glyph | 128 × 128 | 1:1 | Discord's emoji size (`images-emoji.md`) |
| Animated strip | 800 × 96, 15 fps, 2 s | | 30 frames, 76 800 px per frame |

## 4. Text or image?

| Content | Put it in | Why |
| --- | --- | --- |
| Names, numbers, sentences the user reads, copies or searches | Text Display | selectable, searchable, localized per user, read by screen readers, mentions and `<t:unix:R>` timestamps render live |
| Anything clickable | button, select | UD-2 |
| A value as a shape (bar, ring, dial, chart, heat) | image | no component draws shapes |
| Many numbers at a glance (table, leaderboard, stat sheet) | image, with the top values repeated in the alt text | 1 component, 0 characters |
| A small state mark inside a sentence or on a button | custom emoji | text-sized, no attachment |
| Identity (avatar, guild icon, item art) | Thumbnail, or inside a card | |
| A live countdown | `<t:unix:R>` in a Text Display | an image never updates by itself |

The strongest screens use both: the Text Display carries the facts, the image right
under it shows them. A progress strip under "Level 7 · 420 / 1000 XP" is immersive and
still accessible; the same numbers only inside a picture are not.

## 5. A complete first example

`Cargo.toml` (the template already has this):

```toml
noctis = { version = "0.1", features = ["image", "default-font"] }
# "image-gif"   → GIF/APNG output (animated WebP needs only "image")
# "image-emoji" → colour Unicode emoji inside Templates (Twemoji, CC-BY-4.0)
```

The App needs a Renderer: `.theme(noctis::image::theme!())` and/or
`.image(ImageConfig::new())` in `configure` (`images-engine.md` §1 and §8).

```rust
// src/modules/rank/view.rs
use noctis::id::marker::UserMarker;
use noctis::image::{Frame, Node, Style, Template, TemplateView, view};
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Attachment, AttachmentSet, Components, Container, MediaGallery, MediaItem,
    TextDisplay, V2Message, escape_markdown,
};
use serde::{Deserialize, Serialize};

use super::RefreshRank;
use crate::strings::Strings;

/// A bar and nothing else: the numbers live in the message text.
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "rank.progress", version = 1, size = (800, 40), format = Png, module = "rank")]
pub struct ProgressStrip {
    /// 0.0..=1.0
    pub value: f32,
    /// 0xRRGGBB of the fill.
    pub tint: u32,
}

impl TemplateView for ProgressStrip {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center w-full h-full">
                <div tw="flex w-full h-6 rounded-full bg-slate-700 overflow-hidden">
                    <div tw="h-full rounded-full"
                         style={Style::new().width_pct(self.value * 100.0).bg_rgb(self.tint)} />
                </div>
            </div>
        }
    }
}

pub struct RankData {
    pub user: Id<UserMarker>,
    pub name: String,
    pub level: u32,
    pub xp: u64,
    pub next: u64,
    pub bar: Attachment,          // rendered by the handler
}

const TINT: u32 = 0x58_65F2;
const ACCENT: Rgb = rgb!(0x5865_F2);  // the same colour on the Container

pub fn render_rank(d: &RankData, s: &Strings) -> V2Message {
    let mut files = AttachmentSet::new();
    let bar = files.add(d.bar.clone());
    let container = Container::new()
        .accent(ACCENT)
        .push(TextDisplay::new(format!(
            "## {}\n{}",
            escape_markdown(&d.name),
            s.rank.summary(d.level, d.xp, d.next),              // "Level 7 · 420 / 1000 XP"
        )))
        .push(MediaGallery::new([
            MediaItem::new(bar).alt(s.rank.bar_alt(d.xp, d.next)) // "420 of 1000 XP"
        ]))
        .push(ActionRow::buttons([RefreshRank { user: d.user }.build()]));
    V2Message::new(Components::new().push(container)).attachments(files)
}
```

```rust
// src/modules/rank/handlers.rs
#[derive(SlashCommand, Debug)]
#[command("rank", contexts = [Guild], module = "rank")]
pub struct Rank;

impl Rank {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let stats = load(&ctx, ctx.user_id()).await?;                    // all I/O first
        let bar = ctx
            .render(ProgressStrip { value: stats.xp as f32 / stats.next as f32, tint: TINT })
            .await?;                                                     // then the image
        let s = Strings::of(ctx.response_locale());
        ctx.respond(render_rank(&stats.with_bar(bar), &s)).await         // then the View
    }
}
```

The refresh button's handler does the same three steps and ends with
`ctx.update(render_rank(..))`. With unchanged XP the render is a cache hit and the
message keeps its file without an upload.

## 6. Rules that always hold

1. **Data in fields, look in `view`.** Bump `version` whenever the `view` or the
   fields change: the render cache keys on the data and the version, not on the code.
2. **Numbers go through `Style`**, never into a class string:
   `style={Style::new().width_pct(p)}`, not `format!("w-[{p}%]")`. `Style` sets width,
   height, gap, background, text and border colour, opacity, insets (`left`, `top`,
   `right`, `bottom`), corner radius, border width, translation, rotation, scale and
   z-index; every technique in the cookbook is built from those setters and the four
   shapes `shape::Arc` (rings, gauges), `shape::Pie`, `shape::Line` and `shape::Area`
   (line charts, sparklines; `images-engine.md` §3.1).
3. **User text is a text child only** (`{&self.name}`), never a class, never a style.
4. **Localized text is data.** Format labels in the handler from `Strings` and pass
   them as `String` fields; a Template never reads locale files.
5. **Every rendered image has alt text** on its MediaItem or Thumbnail, saying what the
   picture shows in words and numbers (`[ATT-06]`).
6. **No `HashMap`/`HashSet` in Template data** (compile error): `Vec`, `BTreeMap`.
7. **Remote pictures are declared**: `#[asset] avatar: RemoteImage`; tests and previews
   use `RemoteImage::placeholder()`.
8. **Opaque backgrounds for cards.** Users run dark and light themes; a card with its
   own background looks the same in both. Transparent images (strips, badges, glyphs)
   use mid-tone or saturated colours that read on both (`images-ui.md` §7).
9. **`QueueFull` is not an error**: answer with a "busy, try again" notice
   (`images-engine.md` §8).
