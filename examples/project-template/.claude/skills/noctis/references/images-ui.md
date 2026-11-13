# Images in a Components V2 UI

How rendered images become part of a screen: the surfaces and what they cost (§1),
screen layouts with complete code (§2), the handler flow (§3), changing an image on a
click (§4), paginated image pages (§5), animation in a UI (§6), dark and light clients
(§7), limits, performance and caching (§8), images outside an interaction (§9), when
not to use an image (§10), testing (§11), troubleshooting (§12).

Entry and sizes: `images.md`. The Templates used here (`Banner`, `LevelBadge`,
`StatSheet`, `Leaderboard`, `Stepper`, `Board`, …) are in `images-cookbook.md`.
Discord limits are cited by rule id (`limits-cheatsheet.md`); the full catalogue with
sources is noctis-rs's `docs/discord-rules.md`.

## 1. Surfaces and their cost

| Surface | Components | Attachments | Notes |
| --- | --- | --- | --- |
| `MediaGallery` with N items | 1 | N | N is 1..=10 (CV2-10); items do not count as components (CV2-03) |
| `Section` + `Thumbnail` | 2 + texts (1..=3) | 1 | the only place a Thumbnail may be (CV2-05, CV2-07) |
| `File` | 1 | 1 | a download row, not a picture (CV2-12) |
| Text Display with emoji tokens | 1 | 0 | each token counts its characters toward the 4000 (CV2-04); `images-emoji.md` |

Per message: at most 40 components counting every level (CV2-03), 4000 Text Display
characters (CV2-04), 10 attachments (ATT-03), 20 MiB per file unless the interaction
says otherwise (ATT-01) and 25 MiB per request (ATT-02). Noctis checks all of them
before sending; the size budget keeps each render at or under 2 MiB by default
(`images-engine.md` §8), so ten renders fit one request.

**The attachment count is the limit you meet first.** A screen with a banner, a stat
sheet and six Sections with their own Thumbnail is already 8 files. When a screen wants
more than a few small pictures, draw them into one image (a stat sheet instead of four
gauges) or use application emojis for the small marks (`images-emoji.md`).

What Discord documents about the media itself (components reference): a Media Gallery
shows 1 to 10 items; a Thumbnail supports images, including animated GIF and WebP, and
no video; each takes a URL or `attachment://<filename>`, an optional description (alt,
at most 1024 characters) and a `spoiler` flag. Display sizes, cropping and the grid
layout are client behaviour (`images.md` §2).

A `MediaItem` and a `Thumbnail` also accept an `https` URL (`Media::url(..)`), for a
picture that is already hosted. A rendered image is always an attachment.

## 2. Screen layouts

The same six parts make most screens. From top to bottom inside one Container:

```text
┌ Container (accent = the screen's tint) ───────────────┐
│ MediaGallery  banner            1200×300  "where am I" │
│ Section       heading text  │ Thumbnail  badge / avatar│
│ TextDisplay   the facts, in words and numbers          │
│ MediaGallery  the visual     strip / sheet / board     │
│ Separator                                              │
│ ActionRow     the actions                              │
└────────────────────────────────────────────────────────┘
```

Not every screen needs every part. Rules of thumb:

- **One hero image per screen**, at most one banner above it and one strip or sheet
  below the text. More pictures compete and slow the message down.
- **Text before the picture that visualizes it.** The numbers first, the bar under
  them.
- **Buttons last**, directly under what they change.
- **One tint**: the Container accent, the fill of the bars, the ring of the badge and
  the Primary action belong to one colour family. Change the tint with the state
  (neutral, success, danger), not per element.
- **A Thumbnail belongs to its Section's text**: the avatar of the user named there,
  the gauge of the value stated there.

### 2.1 Profile screen (banner, badge, sheet)

```rust
use noctis::id::marker::UserMarker;
use noctis::prelude::*;
use noctis::ui::{
    ActionRow, Attachment, AttachmentSet, Components, Container, MediaGallery, MediaItem,
    Section, Separator, TextDisplay, Thumbnail, V2Message, escape_markdown,
};

use crate::strings::Strings;

const ACCENT: Rgb = rgb!(0x5865_F2);

pub struct StatsData {
    pub user: Id<UserMarker>,
    pub name: String,
    pub level: u32,
    pub rank: u32,
    pub stats: Vec<(String, u32)>,   // also the source of the sheet's alt text
    pub banner: Attachment,          // the three renders, made by the handler
    pub badge: Attachment,
    pub sheet: Attachment,
}

pub fn render_stats(d: &StatsData, s: &Strings) -> V2Message {
    let mut files = AttachmentSet::new();
    let banner = files.add(d.banner.clone());
    let badge = files.add(d.badge.clone());
    let sheet = files.add(d.sheet.clone());

    let listed = d.stats.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", ");
    let container = Container::new()
        .accent(ACCENT)
        .push(MediaGallery::new([MediaItem::new(banner).alt(s.stats.banner_alt())]))
        .push(Section::new(
            [
                TextDisplay::new(format!("## {}", escape_markdown(&d.name))),
                TextDisplay::new(format!("-# {}", s.stats.summary(d.level, d.rank))),
            ],
            Thumbnail::new(badge).alt(s.stats.badge_alt(d.level)),
        ))
        .push(MediaGallery::new([MediaItem::new(sheet).alt(s.stats.sheet_alt(&listed))]))
        .push(Separator::small())
        .push(ActionRow::buttons([RefreshStats { user: d.user }.build()]));
    V2Message::new(Components::new().push(container)).attachments(files)
}
```

Cost: Container 1, gallery 1, Section 4, gallery 1, Separator 1, row 2 = 10 components,
3 attachments, about 40 characters of text.

### 2.2 Other layouts

| Screen | Parts | Notes |
| --- | --- | --- |
| **Rank or XP card** | Text Display ("Level 7 · 420 / 1000 XP") → progress strip → buttons | the first example of `images.md`; the cheapest immersive screen (4 components, 1 file) |
| **Dashboard with gauges** | heading → up to four `Section`s, each with a `RingGauge` Thumbnail and two lines of text → buttons | every Section is 3 to 4 components and 1 file; beyond four, draw one `StatSheet` |
| **Leaderboard** | banner (optional) → `Leaderboard` image → "Page 2 of 7" text → pager row | §5 |
| **Game** | status text ("Your turn, X") → `Board` image → Action Rows with the moves | re-rendered on every move (§4) |
| **Setup flow** | `Stepper` strip → "Step 2 of 3: Role" text → the step's select → Back / Cancel / Next | the strip changes only with the step: always a cache hit |
| **Shop or inventory item** | `Section` (name, price, description + item Thumbnail) per item → pager | 3 items per page keeps it at 3 files; item art as `StaticImage`s inside one Template |
| **Comparison** | text → `MediaGallery::new([before, after])` → buttons | a two-item gallery; the client may crop each tile, so keep the subject centred |
| **Announcement** | banner → text → drawn divider → text → Link button | posted with `ctx.messages().send(..)` from a Command's handler |
| **Poll results** | question text → one image of result rows (the leaderboard's row with a share background) → vote buttons | one image for all options, not one per option |

For a gallery whose item count is known only at run time:

```rust
let gallery = MediaGallery::try_new(
    pages.iter().enumerate().map(|(i, page)| {
        MediaItem::new(files.add(page.clone())).alt(s.album.page_alt(i + 1))
    }),
    limits,                       // ctx.limits(), passed into the render function
)?;                               // Err(UiError::Limits { .. }) for 0 or more than 10 (CV2-10)
```

## 3. The handler flow

```rust
impl Stats {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let p = load(&ctx, ctx.user_id()).await?;                     // 1. all I/O
        let (banner, badge, sheet) = tokio::try_join!(                 // 2. the renders, at once
            ctx.render(Banner { kicker: s.stats.season(), title: s.stats.title() }),
            ctx.render(LevelBadge { level: p.level, caption: s.stats.level(), tint: p.tier_tint }),
            ctx.render(StatSheet { stats: p.sheet_rows(&s) }),
        )?;
        ctx.respond(render_stats(&p.with_images(banner, badge, sheet), &s)).await   // 3. the View
    }
}
```

- **Order.** Load, render, build the message. Render functions stay pure: they receive
  finished `Attachment`s.
- **Several renders in parallel.** `ctx.render` is a future; `tokio::try_join!` runs
  them on the render pool side by side. Each still takes one queue unit.
- **Slow renders are covered.** The auto-defer acknowledges at 2 s and the answer then
  edits the loading message; nothing to do by hand.
- **`QueueFull` is load, not a bug.** Turn it into a notice; let every other
  `RenderError` propagate (it reaches the Render Error Sink and the user gets the
  generic error notice):

  ```rust
  use noctis::image::RenderError;

  let sheet = match ctx.render(sheet).await {
      Ok(sheet) => sheet,
      Err(err) => match err.as_error().downcast_ref::<RenderError>() {
          Some(RenderError::QueueFull) => return ctx.reply_text(keys::COMMON_BUSY).await,
          _ => return Err(err),
      },
  };
  ```

- **A deadline, when you want one.** Dropping the future cancels the render:
  `tokio::time::timeout(Duration::from_secs(8), ctx.render(card)).await`.
- **`ctx.render` exists on `CommandCtx`, `ComponentCtx`, `ModalSubmitCtx` and
  `ExpiredCtx`.** Event handlers and hooks: §9.

## 4. Changing an image on a click

A message is always re-rendered as a whole (MSG-08): the button's handler builds the
new data, renders, and answers with `ctx.update(render_x(..))`.

```rust
#[derive(Button, Debug, Clone, Copy)]
#[button("stats.refresh", defaults(style = Secondary, label = keys::COMMON_REFRESH), module = "stats")]
pub struct RefreshStats {
    pub user: Id<UserMarker>,
}

impl RefreshStats {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.response_locale());
        let p = load(&ctx, self.user).await?;
        let (banner, badge, sheet) = tokio::try_join!(/* the same three renders */)?;
        ctx.update(render_stats(&p.with_images(banner, badge, sheet), &s)).await
    }
}
```

What happens to the files:

- **A render is named by its request** (Template id and version, data, theme, fonts,
  size, format): equal data gives the same file name, whoever asks and whenever.
- **On an edit, a file the message already holds is kept by id** and not uploaded
  again; files the new tree no longer uses are dropped; only new names are uploaded.
  In the profile screen a refresh after an XP gain uploads the sheet only: the banner
  and (same level) the badge stay.
- **So split pictures by how often they change.** A fixed banner, a per-level badge
  and a per-click sheet as three Templates upload less than one big card that changes
  on every click. Do not split further than that: every part is an attachment (§1).
- **Unchanged data re-renders nothing**: the render is a cache hit, the upload is
  skipped, the edit carries only JSON.
- **Quantize what you draw.** A bar drawn from `xp as f32 / next as f32` changes with
  every point; rounding to whole percent (or to the bar's pixel width) makes most
  refreshes a cache hit with the same picture:

  ```rust
  let value = (xp * 100 / next.max(1)) as f32 / 100.0;
  ```

- **State is in the Payload**, never in the picture: the button carries the page, the
  tab, the board (or a Ref to it), and the handler renders from that
  (`routes-payloads-refs.md`).
- **Selects reset to their declared defaults after an update**; set them to the
  current value in the render function (`views-and-recipes.md` §5).
- **Ephemeral messages work the same**: every click is a new interaction that updates
  the message it came from.

A game move, complete. The game lives in `ctx.kv()`; the buttons carry its id and
their cell:

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Game {
    pub cells: [Cell; 9],
    pub players: [u64; 2],     // user ids as u64: `Kv` values never hold `Id<_>`
    pub turn: u8,
}

#[derive(Button, Debug, Clone, Copy)]
#[button("ttt.move", defaults(style = Secondary, label = "·"), module = "ttt")]
pub struct Move {
    pub game: u64,
    pub cell: u8,
}

impl Move {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let key = format!("ttt:{}", self.game);
        let Some(game) = ctx.kv().get::<Game>(&key).await? else {
            return ctx.reply_text(keys::TTT_OVER).await;
        };
        // Payloads are client input: `play` checks the turn and that the cell is free.
        let Some(game) = game.play(ctx.user_id().get(), self.cell) else {
            return ctx.reply_text(keys::TTT_NOT_YOUR_TURN).await;
        };
        ctx.kv().put(&key, &game, Some(Duration::from_secs(3600))).await?;
        let board = ctx.render(Board { cells: game.cells, win: game.winning_line() }).await?;
        ctx.update(render_game(self.game, &game, board, &Strings::of(ctx.response_locale()))).await
    }
}
```

`render_game` puts the status text, the board in a gallery, and three Action Rows of
three `Move` buttons (occupied cells `disabled`, every button a different `cell`).

## 5. Paginated image pages

One page of rows per image; the page number is a Payload field (UD-6).

```rust
const PER_PAGE: usize = 5;      // what one `Leaderboard` image holds

#[derive(Button, Debug, Clone, Copy)]
#[button("board.page", defaults(style = Secondary, label = keys::COMMON_NEXT), module = "board")]
pub struct BoardPage {
    pub guild: Id<GuildMarker>,
    pub page: u16,
}

impl BoardPage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        let s = Strings::of(ctx.guild_locale());                       // a shared message
        let total = count_members(&ctx, self.guild).await?;
        let pages = total.div_ceil(PER_PAGE).max(1) as u16;
        let page = self.page.min(pages - 1);                           // never trust the Payload
        let rows = top_members(&ctx, self.guild, usize::from(page) * PER_PAGE, PER_PAGE).await?;
        let image = ctx.render(leaderboard_of(&rows, &s)).await?;
        ctx.update(render_board(self.guild, page, pages, &rows, image, &s)).await
    }
}

pub fn render_board(
    guild: Id<GuildMarker>, page: u16, pages: u16, rows: &[MemberRow], image: Attachment, s: &Strings,
) -> V2Message {
    let mut files = AttachmentSet::new();
    let image = files.add(image);
    let pager = ActionRow::buttons([
        BoardPage { guild, page: page.saturating_sub(1) }
            .build().label(s.common.previous()).disabled(page == 0),
        BoardPage { guild, page: page + 1 }.build().disabled(page + 1 >= pages),
    ]);
    let container = Container::new()
        .push(MediaGallery::new([MediaItem::new(image).alt(board_alt(rows, s))]))
        .push(TextDisplay::new(format!("-# {}", s.board.page_of(page + 1, pages))))
        .push(pager);
    V2Message::new(Components::new().push(container)).attachments(files)
}
```

- The page size is the **Template's** capacity, not a component budget: the image is
  one component whatever it holds. `Budget` sizes pages of components
  (`views-and-recipes.md` §4), not of pixels.
- Previous and next always carry different `page` values (`0` and `1` on the first
  page), so their ids never collide (CID-02).
- Do not pre-render the neighbours. A page a user never opens costs a render and
  memory; the cache already makes going back free.
- Rows that must be clickable (a "View" button per member) cannot be an image: use
  Sections with accessory buttons, and at most a Thumbnail per row.
- The alt text lists the page's rows in words ("4. Grace 3 900 XP, …"), built from
  the same `rows`.

## 6. Animation in a UI

Animated images autoplay in clients that allow it and loop forever unless the
`AnimationSpec` sets a play count (`.once()`, `.plays(Plays::Times(n))`; whether a
client honours it is client behaviour). Use them where motion says something:

| Moment | Technique |
| --- | --- |
| A value just changed (XP gained, HP lost, vote counted) | frame-driven fill from the old to the new value, rendered with `ctx.render_with(.., animation)` for this one answer (`images-cookbook.md` §2.5) |
| A result is revealed (roll, draw, rank up) | frame-driven view played `.once()` that rests on the result with `.hold_end(..)` (no extra frames drawn) |
| Work continues after the answer | a loading card (`images-cookbook.md` §10.1), replaced by the result |
| Ambience (a live badge, a pulsing status) | a CSS animation in a Thumbnail; keep it tiny (128 to 256 px, 1 to 2 s) |

- **Play once, then settle.** Give the spec `.once()` and a `.hold_end(..)` so the
  file itself ends on the result. The animation belongs to the answer of the interaction
  that caused the change. The next render of the same screen (a refresh, a page turn,
  another user's click) uses the still: the same Template rendered with `ctx.render`.
  A bar that replays its fill every time anyone touches the message is noise.
- **Replacing a loading card** needs a later edit, and handlers never spawn timers
  (UD-7). Edit when the event that finishes the work arrives: the job's completion
  handler or the next interaction calls `ctx.messages().edit(channel, message,
  render_done(..))` for a public message whose ids you stored.
- **Cost.** An animation weighs `ceil(frames / 8)` queue units and three quarters of
  the queue at most hold animations; under load they are the first to get `QueueFull`.
  Fall back to the still instead of failing:

  ```rust
  // `card` is `Clone` (derive it on the Template)
  let bar = match ctx.render_with(card.clone(), RenderOptions::new().animation(spec)).await {
      Ok(bar) => bar,
      Err(err) if matches!(err.as_error().downcast_ref(), Some(RenderError::QueueFull)) => {
          ctx.render(card).await?        // a still weighs 1 unit; a quarter of the queue is reserved for stills
      }
      Err(err) => return Err(err),
  };
  ```

- **Format.** Animated WebP by default (smaller, faster to encode); GIF needs the
  `image-gif` feature and flat colours. Both animate in a Media Gallery and in a
  Thumbnail.
- **When the budget is tight the Renderer degrades by itself**: lower fps, smaller
  size, lower quality, then a still (`ImageConfig::still_fallback`, on by default).
  `ctx.render_report(t, options)` tells what was encoded (`report.settings_used`,
  `report.format`).

## 7. Dark and light clients

The bot cannot know the viewer's theme, and one public message is seen in both.

- **Cards bring their own background.** An opaque dark (or light) surface with its own
  text colours looks identical everywhere. Round the corners (`rounded-3xl`) and leave
  the outside transparent (PNG, WebP), or go full-bleed (JPEG, animations).
- **Transparent pictures use colours that survive both**: saturated tints and mid
  greys (`slate-400` to `slate-600`) for tracks and inactive parts. Never white or
  near-black text on a transparent root.
- **Test both**: paste the snapshot PNG onto a dark and a light background before
  shipping a transparent Template (§11).
- **Match the Container.** Inside a Container the picture sits on the Container's
  background, which differs from the chat background, again per theme. An image that
  tries to blend in by painting "Discord's" background colour is wrong in at least one
  of the four cases; a visible card edge is better than a near-miss.

## 8. Limits, performance, caching

**Per render** (defaults; `images-engine.md` §8): at most 4 MP per job, 1 MP per
animation frame, 90 frames, 6 s; output at or under `ImageConfig::size_target` (2 MiB)
and the interaction's upload limit. Over a cap is `RenderError::OverCap` before
anything is drawn; over the budget the Renderer degrades, then fails with
`RenderError::OverBudget`.

**Indicative costs**, measured for the cookbook's Templates on a small cloud machine
(yours differ; watch `noctis_render_duration_seconds{app, template}`):

| Picture | Render | Bytes |
| --- | --- | --- |
| Strip, divider, emoji glyph (PNG) | under 1 ms | 0.2 to 1 KB |
| Badge, gauge, tile 256 × 256 (PNG) | 2 to 5 ms | 2 to 7 KB |
| Chart, leaderboard, banner 900 to 1200 px wide | 10 to 25 ms | 15 to 35 KB |
| Animated strip 800 × 96, 30 frames | about 50 ms | 5 KB WebP, 27 KB GIF |
| Loading card 600 × 120, 30 frames | about 50 ms | 120 KB WebP |

Flat shapes in PNG are tiny; gradients, shadows and photos are where JPEG or WebP pay
off. Bytes matter twice: upload time on your side, download on the user's phone.

**What makes renders cheap:**

1. **Cache hits.** Identical requests render once and are then served from the
   `RenderCache` (64 MiB in memory by default, `AppBuilder::render_cache(..)` to share
   one across processes). Make requests identical on purpose: quantize ratios, round
   times to the minute, format numbers before they reach the Template, leave out
   fields the view does not draw (a user id in the data makes every user a new key).
2. **Single flight.** Ten users pressing the same button at once share one render.
3. **Kept attachments.** An edit uploads only files the message does not hold (§4).
4. **Small canvases.** Halving both sides quarters the work. A Thumbnail never needs
   more than 256 × 256.
5. **Remote images at the size you draw**, cached in the `AssetStore` for a day by
   default. A leaderboard page fetches its five avatars once.
6. **Fewer frames.** 12 to 15 fps, 1.5 to 2 s.

**What to tune** (`ImageConfig`, in `configure`):

```rust
use noctis::image::{ImageConfig, RenderCaps};

builder.image(
    ImageConfig::new()
        .threads(4)                     // render threads; default: half the cores
        .queue_capacity(128)            // admission units; a full queue is QueueFull, never a wait
        .size_target(512 * 1024)        // per-file byte budget; default 2 MiB
        .caps(RenderCaps::new().frames(60)),
);
```

A lower `size_target` is a cheap way to keep mobile users happy: the Renderer shrinks
what does not fit. Every App on one `Runtime` must pass the same `threads` and
`queue_capacity`.

**Rate limits still apply.** Each click is one interaction response; an edit loop that
redraws a message on a timer is not a pattern (UD-7) and runs into Discord's rate
limits. Redraw on interactions and on real events.

## 9. Outside an interaction

Every context renders: `EventCtx`, `HookCtx` and the built `App` have the same
`render(t)`, `render_with(t, RenderOptions)` and `render_report(t, RenderOptions)` as
the interaction contexts, with the same errors, cache, span and metrics. There is no
interaction to report an upload limit, so the budget is `Limits::attachment_bytes`
(20 MiB by default; the Renderer still fits `size_target`). Post the result with
`ctx.messages().send(channel, view)`.

A welcome card on `MemberAdd`:

```rust
use noctis::events::MemberAdd;
use noctis::image::RemoteImage;
use noctis::prelude::*;
use noctis::ui::{AttachmentSet, Components, MediaGallery, MediaItem, V2Message};

#[derive(EventHandler)]
#[event(module = "welcome")]
pub struct Welcome;

impl Welcome {
    #[handler]
    async fn handle(self, ctx: EventCtx, ev: MemberAdd) -> HandlerResult<()> {
        let card = WelcomeCard { name: ev.user.name.clone(), avatar: RemoteImage::avatar(&ev.user, 256) };
        let mut files = AttachmentSet::new();
        let image = files.add(ctx.render(card).await?);
        let msg = V2Message::new(Components::new().push(
            MediaGallery::new([MediaItem::new(image).alt("Welcome card")]),
        ))
        .attachments(files);
        ctx.messages().send(WELCOME_CHANNEL, msg).await?;
        Ok(())
    }
}
```

A scheduled leaderboard: start the loop from an `AppStarted` hook and render with the
hook's context (`HookCtx` is a cheap clone, so move it into the task):

```rust
#[handler]
async fn handle(self, ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
    tokio::spawn(async move {
        let mut every = tokio::time::interval(Duration::from_secs(3600));
        loop {
            every.tick().await;
            let board = load_board(&ctx.kv()).await;               // your data
            match ctx.render(board).await {
                Ok(image) => { /* ctx.messages().send(BOARD_CHANNEL, view_with(image)).await */ }
                Err(err) => tracing::warn!(%err, "leaderboard render failed"),
            }
        }
    });
    Ok(())
}
```

Outside any handler, `app.render(t)` (and `render_with`, `render_report`) renders on
the built App before `app.run()` or in a Shutdown Task; its future borrows nothing, so
`tokio::spawn(app.render(card))` works. Without a Renderer every call is
`RenderError::NoRenderer`.

The render is awaited where it is called: no new lane, no detached task. When the App
shuts down, an event handler or hook still rendering is dropped with the rest of its
work, which cancels the render. A spawned task outlives that, so stop it yourself
(a `ShutdownStarted` hook or a Shutdown Task). `noctis::image::unstable` is only for
writing your own Renderer (`adapters.md` §2b), never for rendering.

## 10. When not to use an image

- **The user must act on it**: links, commands to type, codes to copy, choices. Those
  are components and text (UD-2).
- **It is a sentence.** Text in a picture is not selectable, not searchable, not
  translated by the client, and gets small on phones.
- **It changes every second.** Use `<t:unix:R>` for time; redraw on interactions.
- **It is a list of things to click.** Sections with accessory buttons, or a select.
- **It is a single state mark** inside a line of text or on a button: an emoji
  (`images-emoji.md`), not a 1-file Thumbnail.
- **It would be the eleventh attachment**, or the screen is already heavy.
- **A Separator or a heading does the job.** Decoration costs a component, a file and
  a download.
- **The channel is a log.** Log lines are read in bulk and searched: text.

## 11. Testing

Pixel snapshots catch a broken layout before users do (`testing.md` §6 has every rule):

```rust
use noctis::App;
use noctis::image::RemoteImage;
use noctis::testkit::{TestApp, assert_render_snapshot};
use noctis_bot_template::{configure, modules::stats::{Leaderboard, BoardRow, RingGauge}};

#[tokio::test(flavor = "multi_thread")]
async fn stats_images_match_their_snapshots() -> anyhow::Result<()> {
    let app = TestApp::build(configure(&mut App::builder())).await?;

    // the edges of the value range: an empty, a tiny and a full gauge
    for (name, value) in [("ring_0", 0.0), ("ring_3", 0.03), ("ring_100", 1.0)] {
        let gauge = RingGauge { value, label: format!("{:.0}%", value * 100.0), caption: "CPU".into(), tint: 0x34_D399 };
        assert_render_snapshot!(app.renderer(), name, gauge, tolerance = 0.01);
    }

    // the worst-case text: the longest name, the largest number, a full page
    let rows: Vec<BoardRow> = (1..=5)
        .map(|rank| BoardRow {
            rank,
            name: "W".repeat(32),
            score_label: "999 999 999 XP".into(),
            share: 1.0 / rank as f32,
        })
        .collect();
    let board = Leaderboard {
        title: "Top members".into(),
        avatars: vec![RemoteImage::placeholder(); rows.len()],        // no network
        rows,
    };
    assert_render_snapshot!(app.renderer(), "board_worst_case", board, tolerance = 0.01);

    app.stop().await?;
    Ok(())
}
```

- Snapshot the **edges**: 0 %, a few percent, 100 %, an empty list, a full page, the
  longest label of every locale. A layout that survives those survives production.
- Look at the PNGs (`tests/snapshots/<name>.png`) on a dark and on a light background
  before committing them.
- A snapshot is the still: an animated Template is compared at `Frame::STILL`. Test
  the motion's maths as plain functions (`progress(frame, 0.5)` at the first, middle
  and last frame).
- The message around the image is tested like any View: run the Command on the
  `TestApp`, assert on the response's components and attachments (`testing.md` §3 and
  §4), or unit-test `render_x` with `Attachment::new(vec![0u8], "png")?` standing in
  for a render.
- Unknown runtime classes (`Classes::dynamic`) fail renders on a test Runtime, so a
  typo in a dynamic class fails the snapshot test instead of logging in production.

## 12. Troubleshooting

| Symptom | Cause | Fix |
| --- | --- | --- |
| `RenderError::NoRenderer` | no `.theme(..)` / `.image(..)` on the builder | add one in `configure` |
| `RenderError::UnknownTemplate` | the Template is not registered | `module = ".."` in `#[template(..)]`, or `Module::template::<T>()` |
| the picture did not change after editing `view` | the cache served the old render | bump `version` in `#[template(..)]` |
| `[TW-01] unknown Tailwind class` | a class outside the grammar (`ring-2`, `space-x-4`, `sm:z-10`, a v3 name) or a missing theme token | `images-engine.md` §4; add the token to `noctis-theme.css` |
| a gradient stop with a theme colour fails (`from-brand`) | the token is not in `noctis-theme.css` | declare `--color-brand` there |
| text draws as boxes | no glyph: emoji without `image-emoji`, a script the fonts lack | enable the feature, or register a font (`images-engine.md` §6) |
| `[IMG-01] no font is registered` at build | Templates but no font | the `default-font` feature or `ImageConfig::font` |
| an avatar is a grey square | the fetch fell back (a warning on target `noctis::image` says why), or `RemoteImage::placeholder()` | check the origin allow-list and the URL; in tests this is expected |
| `RenderError::UndeclaredImage` | the view draws a `RemoteImage` no `#[asset]` field holds (often one inside a nested struct) | move it into an `#[asset]` field of the Template |
| a child is stretched or squashed | a flex parent stretches children without a size | give the `<img>` or box `w-*`/`h-*`, or put it in an `items-center` row |
| the fill overflows its rounded track | no clipping | `overflow-hidden` on the track |
| an overlay is hidden | painted before the thing it should cover | `z-10` on it (it must be `relative` or `absolute`), or move it later among its siblings |
| long names break the row | unbounded text | `truncate` on a `flex-1` span, or `line-clamp-2` |
| the layout is empty or collapsed | the root has no size | `w-full h-full` on the root |
| `RenderError::SizeMismatch` | an animation frame's root has an explicit pixel size | keep the root `w-full h-full` |
| `RenderError::OverCap` | too many pixels, frames or seconds, or too many remote images | shrink the Template, lower fps or duration, or raise `RenderCaps` |
| the animation arrives as a still | the size budget's still fallback | fewer frames, a smaller size, or a higher `size_target`; check `ctx.render_report` |
| an animation shows only its first frame in tests | snapshots are stills | expected (§11) |
| `[ATT-06]` on send | a rendered image without `.alt(..)` on its MediaItem or Thumbnail | add the alt text there (the `Attachment`'s own alt does not count) |
| `ResponderError::Invalid` naming `attachments` | more than 10 used files (ATT-03) | merge pictures into one Template, or use emojis |
| the picture is blurry on desktop | the canvas is smaller than its display size | a wider `size` (strips at 800 px or more) |
| the picture is unreadable on phones | text too small for a scaled-down image | larger text classes, fewer rows per image |
| "Busy, try again" too often | the render queue is full | more `threads`, a larger `queue_capacity`, fewer or shorter animations, more cache hits (§8) |

Compile errors and rule ids in full: `troubleshooting.md`.
