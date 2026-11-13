# Image cookbook: drawing techniques

Working Templates for the pictures a bot UI needs. Every block compiles against Noctis
0.1 with the template's theme (`bg-surface`, `text-muted`, `bg-brand`, `text-brand-soft`
from `noctis-theme.css`) and the `default-font` feature. Entry and sizes: `images.md`;
placing them in a message: `images-ui.md`; exact engine rules: `images-engine.md`.

Covers: the toolbox (§1), bars (§2), gauges (§3), thumbnails (§4), lists and cards (§5),
charts (§6), banners and dividers (§7), boards (§8), steps and countdowns (§9), motion
(§10), text, emoji and fonts inside images (§11).

Every block assumes:

```rust
use noctis::image::{Frame, Node, RemoteImage, Style, Template, TemplateView, tw, view};
use serde::{Deserialize, Serialize};
```

and that the Template is registered: add `module = "<feature>"` to its `#[template(..)]`
(discovered), as the first example of `images.md` does. Ids in this file use the prefix
`ui.`; give yours the feature's name (`rank.progress`).

## 1. The toolbox

There is no canvas API, no path drawing and no `<svg>`. A picture is a tree of boxes,
text and images, laid out with flexbox or grid and painted in tree order. That is
enough for almost everything, with these moves:

| To get | Do |
| --- | --- |
| A value as a length | a track `div` with a fill child: `style={Style::new().width_pct(v * 100.0)}` (or `height_pct` in an `items-end` row) |
| A value as a colour | `Style::new().bg_rgb(0xRRGGBB)`, `.color_rgb(..)`, `.border_color_rgb(..)` computed from the data |
| A value as intensity | one colour with `Style::new().opacity(v)` over a dark cell |
| A value as an angle | a square wrapper rotated with `Style::new().rotate_deg(a)`; its child sits at the top centre, so it travels on a circle (§3) |
| N of M | `for i in 0..m { .. }` with a class chosen per item: `tw={if i < n { tw!("..") } else { tw!("..") }}` |
| Equal parts | `flex-1` children in a `flex gap-*` row, or `grid grid-cols-N` |
| A value as a position | a `relative` track with an `absolute` marker: `style={Style::new().left_pct(v * 100.0).translate_px(-8, 0)}` (half the marker's width back, so it centres on the value) |
| One thing over another | `relative` parent, `absolute` child with `inset-0`, `top-*`, `left-*`, `right-*`, `bottom-*` (literal or `top-[140px]`) or, when the offset is data, `Style::new().left_px(x).top_px(y)` (`_pct` for percent); a later sibling paints over an earlier one unless `Style::new().z_index(n)` lifts it |
| A size, radius or border from data | `Style::new().scale(f)` (about the centre, no relayout), `.rounded_px(r)`, `.border_px(w)` |
| A clipped shape | `overflow-hidden` plus `rounded-*` on the parent |
| A circle, pill, ring | `rounded-full`; a ring is `rounded-full border-[12px]` with no background |
| Overlap | negative margins: `-ml-6` |
| A reusable part | a function `fn row(r: &Row) -> Node { view! { .. } }` called as `{row(r)}` |
| Long text that must not break the layout | `truncate` on a `flex-1` span (one line, ellipsis) or `line-clamp-2` |
| Depth | `shadow-lg`, `bg-white/10` overlays, `bg-linear-to-br from-indigo-600 to-fuchsia-500` (gradient stops take palette colours only, never theme tokens) |

`Style` has these setters (`images-engine.md` §4): `width_px`/`width_pct`,
`height_px`/`height_pct`, `gap_px`, `bg_rgb`, `color_rgb`, `border_color_rgb`,
`opacity`, `left_px`/`left_pct`, `top_px`/`top_pct`, `right_px`/`right_pct`,
`bottom_px`/`bottom_pct`, `rounded_px`, `border_px`, `translate_px`, `rotate_deg`,
`scale` and `z_index`. Anything else that varies is a choice between literal class
lists.
Percentages are clamped to `0..=100` and a NaN is ignored, so a division by zero cannot
break a render; still, compute ratios with a guard (`max.max(1)`).

**Colour as data.** Pass tints as `u32` fields (`0xRRGGBB`) when the handler decides
them (rarity, team, status) and keep fixed colours as classes. Reuse the same value for
the Container accent: `Rgb::try_from(tint)`.

## 2. Bars

The bare strip (a track and a fill, 800 × 40) is the first example of `images.md` §5.
Put it directly under the Text Display that states the numbers.

### 2.1 Labelled bar

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.xpbar", version = 1, size = (800, 96), format = Png)]
pub struct XpBar {
    pub left: String,    // "Level 7"        (localized by the handler)
    pub right: String,   // "420 / 1000 XP"
    pub value: f32,      // 0.0..=1.0
}

impl TemplateView for XpBar {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col justify-center gap-3 w-full h-full px-6 bg-surface rounded-2xl">
                <div tw="flex justify-between text-2xl font-semibold text-white">
                    <span>{&self.left}</span>
                    <span tw="text-muted">{&self.right}</span>
                </div>
                <div tw="flex w-full h-5 rounded-full bg-slate-700 overflow-hidden">
                    <div tw="h-full rounded-full bg-linear-to-r from-indigo-500 to-fuchsia-400"
                         style={Style::new().width_pct(self.value * 100.0)} />
                </div>
            </div>
        }
    }
}
```

`overflow-hidden` on the track keeps a tiny fill (2 %) inside the rounded end instead
of drawing a squashed pill.

### 2.2 Segmented bar

Hearts, quest steps, charges, "3 of 8": discrete counts read better as segments.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.segments", version = 1, size = (800, 40), format = Png)]
pub struct Segments {
    pub filled: u8,
    pub total: u8,     // keep it small: 3..=20
}

impl TemplateView for Segments {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center gap-2 w-full h-full">
                for i in 0..self.total {
                    <div tw={if i < self.filled { tw!("flex-1 h-5 rounded-md bg-emerald-400") }
                             else { tw!("flex-1 h-5 rounded-md bg-slate-700") }} />
                }
            </div>
        }
    }
}
```

A partly filled last segment: give each segment a track and a fill child, and set the
fill's `width_pct` to `((value * total) - i).clamp(0.0, 1.0) * 100.0`.

### 2.3 Stat sheet (several bars)

One image, any number of rows, one component. Each row is label, bar, value.

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Stat {
    pub label: String,
    pub value_label: String,
    pub value: f32,    // 0.0..=1.0
    pub tint: u32,     // 0xRRGGBB
}

#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.stats", version = 1, size = (800, 320), format = Png)]
pub struct StatSheet {
    pub stats: Vec<Stat>,   // 4 rows fit 320 px; add 64 px of height per extra row
}

impl TemplateView for StatSheet {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col justify-center gap-5 w-full h-full p-8 bg-surface rounded-3xl">
                for s in &self.stats {
                    <div tw="flex items-center gap-5">
                        <span tw="w-40 text-2xl font-semibold text-muted">{&s.label}</span>
                        <div tw="flex flex-1 h-5 rounded-full bg-slate-800 overflow-hidden">
                            <div tw="h-full rounded-full"
                                 style={Style::new().width_pct(s.value * 100.0).bg_rgb(s.tint)} />
                        </div>
                        <span tw="w-24 text-right text-2xl font-bold text-white">{&s.value_label}</span>
                    </div>
                }
            </div>
        }
    }
}
```

The fixed `w-40` and `w-24` columns align the bars whatever the label lengths are; pick
them for the longest label of every locale you ship. A Template has one size: for a
variable row count either pad to a fixed count, or declare one Template per count
(`StatSheet4`, `StatSheet8`) sharing one row function.

### 2.4 A bar behind a row

A share drawn as the row's own background (leaderboards, poll results): §5.1.

### 2.5 Animated fill

The view gets a `Frame { index, count }` and draws that moment. Ease towards the end
value over the frames, then let the file rest on the full bar: play it once and hold
the last frame (`.once().hold_end(..)` below). A looping bar that snaps back to zero
the moment it is full reads as broken.

```rust
/// 0.0 → 1.0 over the first `run` share of the frames (ease-out cubic), then held
/// (`run = 1.0` eases over every frame).
/// `Frame::STILL` is `{ index: 0, count: 1 }`, so a still shows the end state.
fn progress(frame: Frame, run: f32) -> f32 {
    let t = (frame.index + 1) as f32 / (frame.count as f32 * run).max(1.0);
    1.0 - (1.0 - t.min(1.0)).powi(3)
}

#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.xpgain", version = 1, size = (800, 96), format = Png)]
pub struct XpGain {
    pub left: String,
    pub right: String,   // "+180 XP"
    pub from: f32,       // the bar before the gain
    pub to: f32,         // and after it
}

impl TemplateView for XpGain {
    fn view(&self, frame: Frame) -> Node {
        let value = self.from + (self.to - self.from) * progress(frame, 1.0);
        view! {
            <div tw="flex flex-col justify-center gap-3 w-full h-full px-6 bg-surface">
                <div tw="flex justify-between text-2xl font-semibold text-white">
                    <span>{&self.left}</span>
                    <span tw="text-muted">{&self.right}</span>
                </div>
                <div tw="flex w-full h-5 rounded-full bg-slate-700 overflow-hidden">
                    <div tw="h-full rounded-full bg-indigo-500"
                         style={Style::new().width_pct(value * 100.0)} />
                </div>
            </div>
        }
    }
}
```

The Template declares no `animation(..)`, so the same struct serves both cases:

```rust
use std::time::Duration;
use noctis::image::{AnimatedFormat, AnimationSpec, RenderOptions};

let card = XpGain { left, right, from: before, to: after };
let bar = if before == after {
    ctx.render(card).await?                    // plain view or refresh: a still PNG of the end state
} else {
    // right after a gain: the same Template, animated for this call (1 s at 15 fps =
    // 15 frames), played once, then resting on the full bar for 3 s
    let spec = AnimationSpec::new(Duration::from_secs(1), 15, AnimatedFormat::DEFAULT)
        .once()
        .hold_end(Duration::from_secs(3));
    ctx.render_with(card, RenderOptions::new().animation(spec)).await?
};
```

Put `animation(duration_ms = 1000, fps = 15, plays = 1, hold_end_ms = 3000)` in
`#[template(..)]` instead when the picture is always animated; add `format = Gif`
there (or `AnimatedFormat::Gif` in the spec) for a GIF, which needs the `image-gif`
feature.

- **WebP or GIF.** The default animated WebP is smaller and encodes faster: this strip
  measured about 5 KB as WebP and 27 KB as GIF. Choose GIF only when the file must be a
  `.gif` (re-used outside Discord, or an animated application emoji).
- **Flat colours for GIF.** GIF has 256 colours per frame and one-bit transparency:
  no gradients, no soft shadows, an opaque full-bleed root (no rounded transparent
  corners). The block above uses a square `bg-surface` root for that reason.
- **Frames cost.** Queue weight is `ceil(frames / 8)` and bytes grow with frames:
  12 to 15 fps for 1.5 to 2 s is enough for a bar. 30 frames of 800 × 96 rendered in
  about 50 ms on a small cloud machine; measure yours (`images-ui.md` §8).
- **Play count and holds.** `.plays(Plays::Times(n))` / `.once()` (default
  `Plays::Forever`; `plays = n` on the attribute, 1..=65535) is written into the file
  as `n` plays in total, in every format. `.hold_start(d)` / `.hold_end(d)`
  (`hold_start_ms` / `hold_end_ms`) show the first / last frame `d` longer: no frame
  is drawn for a hold, so it costs no queue weight, and the duration cap counts
  `duration + holds` (6 s by default). Frames that come out pixel-identical are
  written as one longer frame, so an eased bar that already reached its end costs no
  bytes either. Whether a client honours the play count is client behaviour: a
  client may still loop it or not play it at all, so the held end state must read on
  its own. To be sure the next view is static, swap the animation for a still on the
  next interaction (`images-ui.md` §6).

## 3. Gauges

### 3.1 Ring gauge

Forty ticks on a circle; the lit share is the value. Each tick is a small bar at the
top centre of a square wrapper, and the wrapper is rotated.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.ring", version = 1, size = (256, 256), format = Png)]
pub struct RingGauge {
    pub value: f32,        // 0.0..=1.0
    pub label: String,     // "72%"
    pub caption: String,   // "CPU"
    pub tint: u32,
}

const TICKS: u16 = 40;

impl TemplateView for RingGauge {
    fn view(&self, _frame: Frame) -> Node {
        let lit = (self.value.clamp(0.0, 1.0) * f32::from(TICKS)).round() as u16;
        view! {
            <div tw="relative flex items-center justify-center w-full h-full bg-surface rounded-3xl">
                for i in 0..TICKS {
                    <div tw="absolute inset-4 flex justify-center"
                         style={Style::new().rotate_deg(f32::from(i) * 360.0 / f32::from(TICKS))}>
                        <div tw="w-2 h-7 rounded-full"
                             style={Style::new().bg_rgb(if i < lit { self.tint } else { 0x1E_293B })} />
                    </div>
                }
                <div tw="flex flex-col items-center">
                    <span tw="text-6xl font-bold text-white">{&self.label}</span>
                    <span tw="text-xl text-muted">{&self.caption}</span>
                </div>
            </div>
        }
    }
}
```

At 256 × 256 it is a Thumbnail next to the text that explains it; at 512 × 512 with
larger text classes it stands alone. More ticks (60, `w-1`) look finer; fewer and
thicker (12, `w-4 h-8`) look like a battery. The label repeats the value, so the
Thumbnail's alt text is the same sentence ("CPU at 72 %").

### 3.2 Dial with a needle

A half circle of ticks coloured by zone and a needle whose angle is the value: latency,
load, risk, "how full".

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.dial", version = 1, size = (320, 208), format = Png)]
pub struct Dial {
    pub value: f32,      // 0.0..=1.0, left to right
    pub label: String,   // "30 ms"
}

const DIAL_TICKS: u8 = 25;   // every 7.5° from -90° to +90°

impl TemplateView for Dial {
    fn view(&self, _frame: Frame) -> Node {
        let value = self.value.clamp(0.0, 1.0);
        let zone = |i: u8| match f32::from(i) / f32::from(DIAL_TICKS - 1) {
            p if p < 0.6 => 0x34_D399,    // fine
            p if p < 0.85 => 0xFB_BF24,   // warning
            _ => 0xF4_3F5E,               // critical
        };
        view! {
            <div tw="relative flex items-end justify-center w-full h-full pb-3 bg-surface rounded-3xl">
                for i in 0..DIAL_TICKS {
                    <div tw="absolute top-6 left-8 flex justify-center w-64 h-64"
                         style={Style::new().rotate_deg(-90.0 + f32::from(i) * 7.5)}>
                        <div tw="w-2 h-6 rounded-full" style={Style::new().bg_rgb(zone(i))} />
                    </div>
                }
                // the needle: same wrapper, one angle, a longer bar that ends at the centre
                <div tw="absolute top-6 left-8 flex justify-center w-64 h-64"
                     style={Style::new().rotate_deg(-90.0 + value * 180.0)}>
                    <div tw="mt-10 w-2 h-24 rounded-full bg-white" />
                </div>
                <div tw="absolute top-[140px] left-[148px] w-6 h-6 rounded-full bg-white" />
                <span tw="text-3xl font-bold text-white">{&self.label}</span>
            </div>
        }
    }
}
```

The geometry: a 256 px wrapper (`w-64 h-64`) at `top-6 left-8` of a 320 px wide canvas
is centred, its centre is at (160, 152), and a rotation turns around that centre. The
needle starts 40 px below the wrapper's top (`mt-10`) and is 96 px long (`h-24`), so it
ends 8 px short of the centre, under the hub (a 24 px circle at 148, 140). Keep those
numbers consistent when you resize. An animated needle is the same view with
`value * progress(frame, 0.6)` (§2.5).

## 4. Thumbnails: badges, avatars, tiles

A Thumbnail is small everywhere: one shape, at most one short number, strong contrast.

### 4.1 Level badge

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.badge", version = 1, size = (256, 256), format = Png)]
pub struct LevelBadge {
    pub level: u32,
    pub caption: String,   // "level"
    pub tint: u32,         // tier colour
}

impl TemplateView for LevelBadge {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center justify-center w-full h-full">
                <div tw="flex flex-col items-center justify-center w-60 h-60 rounded-full border-[12px] bg-slate-900 shadow-lg"
                     style={Style::new().border_color_rgb(self.tint)}>
                    <span tw="text-8xl font-black text-white">{self.level}</span>
                    <span tw="text-2xl font-semibold uppercase tracking-widest"
                          style={Style::new().color_rgb(self.tint)}>{&self.caption}</span>
                </div>
            </div>
        }
    }
}
```

The root has no background, so the PNG is transparent around the disc and sits on any
client theme. A four-digit level needs `text-6xl`: choose the class from the digit
count with a `tw!` branch.

### 4.2 Avatar with a status dot

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.avatar", version = 1, size = (256, 256), format = Png)]
pub struct StatusAvatar {
    #[asset]
    pub avatar: RemoteImage,
    /// 0xRRGGBB, chosen by the handler: e.g. 0x23A55A online, 0xF0B232 idle, 0xF23F43 busy.
    pub status: u32,
}

impl TemplateView for StatusAvatar {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="relative flex w-full h-full">
                <img src={&self.avatar} tw="w-full h-full rounded-full" />
                <div tw="absolute right-2 bottom-2 w-16 h-16 rounded-full border-8 border-slate-900"
                     style={Style::new().bg_rgb(self.status)} />
            </div>
        }
    }
}
```

The same overlay move makes rank ribbons, "new" dots, team colours and level chips on
an avatar. Building the `RemoteImage` (and what happens when the fetch fails): §11 and
`images-engine.md` §7.

### 4.3 Sparkline tile

A trend in a Thumbnail: one large figure, a row of thin columns.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.spark", version = 1, size = (256, 256), format = Png)]
pub struct Sparkline {
    pub label: String,       // "+12%"
    pub points: Vec<u32>,    // oldest first; 8 to 30 points
    pub up: bool,
}

impl TemplateView for Sparkline {
    fn view(&self, _frame: Frame) -> Node {
        let max = self.points.iter().copied().max().unwrap_or(0).max(1);
        let tint = if self.up { 0x34_D399 } else { 0xF4_3F5E };
        view! {
            <div tw="flex flex-col gap-3 w-full h-full p-5 bg-surface rounded-3xl">
                <span tw="text-5xl font-black" style={Style::new().color_rgb(tint)}>{&self.label}</span>
                <div tw="flex flex-1 items-end gap-1">
                    for p in &self.points {
                        <div tw="flex-1 rounded-sm"
                             style={Style::new().height_pct(*p as f32 / max as f32 * 100.0).bg_rgb(tint)} />
                    }
                </div>
            </div>
        }
    }
}
```

A connected line cannot be drawn (no paths); columns or dots are the sparkline. For
dots, replace each column by a `relative flex-1` cell holding an `absolute w-2 h-2
rounded-full` dot placed with `Style::new().bottom_pct(*p as f32 / max as f32 * 90.0)` (90 so the
top dot stays inside the cell).

## 5. Lists and cards

### 5.1 Leaderboard

```rust
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BoardRow {
    pub rank: u32,
    pub name: String,
    pub score_label: String,   // "9 000 XP", formatted for the locale by the handler
    pub share: f32,            // of the leader's score, 0.0..=1.0
}

#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.board", version = 1, size = (900, 600), format = Png)]
pub struct Leaderboard {
    pub title: String,
    pub rows: Vec<BoardRow>,          // at most 5 at this size: one page
    #[asset]
    pub avatars: Vec<RemoteImage>,    // same order and length as `rows`
}

fn medal(rank: u32) -> u32 {
    match rank {
        1 => 0xFB_BF24,
        2 => 0xCB_D5E1,
        3 => 0xD9_7706,
        _ => 0x33_4155,
    }
}

impl TemplateView for Leaderboard {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col gap-3 w-full h-full p-8 bg-surface rounded-3xl">
                <span tw="text-4xl font-bold text-white">{&self.title}</span>
                for (row, avatar) in self.rows.iter().zip(&self.avatars) {
                    <div tw="relative flex items-center gap-5 h-20 px-5 rounded-2xl bg-slate-900 overflow-hidden">
                        // the share, as the row's own background: painted first, so under the rest
                        <div tw="absolute left-0 top-0 h-full bg-brand/20"
                             style={Style::new().width_pct(row.share * 100.0)} />
                        <div tw="flex items-center justify-center w-12 h-12 rounded-full text-2xl font-black text-slate-900"
                             style={Style::new().bg_rgb(medal(row.rank))}>{row.rank}</div>
                        <img src={avatar} tw="w-14 h-14 rounded-full" />
                        <span tw="flex-1 text-3xl font-semibold text-white truncate">{&row.name}</span>
                        <span tw="text-3xl font-bold text-brand-soft">{&row.score_label}</span>
                    </div>
                }
            </div>
        }
    }
}
```

- `truncate` on the `flex-1` name keeps a 32-character nickname from pushing the score
  out of the row.
- Remote images live in `#[asset]` fields of the Template itself (`RemoteImage`,
  `Option<RemoteImage>`, `Vec<RemoteImage>`), not inside nested structs: keep a parallel
  `Vec<RemoteImage>` and `zip` it. At most 16 distinct remote images per render by
  default (`RemoteConfig::max_per_job`).
- One page per image; page with buttons (`images-ui.md` §5). The viewer's own row,
  highlighted, is one more `tw!` branch on `row.rank == self.me`.
- The alt text names the top entries: "Top members: 1. Deniz 9 000 XP, 2. …".

### 5.2 Avatar stack

Party members, voters, "who is in": overlapping circles and a "+N" chip.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.party", version = 1, size = (600, 96), format = Png)]
pub struct Party {
    #[asset]
    pub members: Vec<RemoteImage>,   // the first 5
    pub extra: u32,                  // how many more
}

impl TemplateView for Party {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center w-full h-full pl-6">
                for member in &self.members {
                    <img src={member} tw="-ml-6 w-24 h-24 rounded-full border-4 border-slate-900" />
                }
                if self.extra > 0 {
                    <div tw="-ml-6 flex items-center justify-center w-24 h-24 rounded-full border-4 border-slate-900 bg-slate-700 text-3xl font-bold text-white">
                        {format!("+{}", self.extra)}
                    </div>
                }
            </div>
        }
    }
}
```

### 5.3 Profile and rank cards

The template's `ProfileCard` (`src/modules/profile/view.rs`) is the pattern: an avatar
with a ring, a name, a subtitle, a two-sided label row and a bar, on an opaque rounded
surface at 934 × 282. Extend it with the parts above: a `LevelBadge`-style disc in the
right corner (`absolute`), a row of tinted chips for roles, a `StatusAvatar` overlay.

## 6. Charts

### 6.1 Column chart

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.chart", version = 1, size = (900, 420), format = Png)]
pub struct BarChart {
    pub title: String,
    pub bars: Vec<(String, u32)>,   // (label, value); 3 to 12 bars
}

impl TemplateView for BarChart {
    fn view(&self, _frame: Frame) -> Node {
        let max = self.bars.iter().map(|b| b.1).max().unwrap_or(0).max(1);
        view! {
            <div tw="flex flex-col gap-4 w-full h-full p-8 bg-surface rounded-3xl">
                <span tw="text-3xl font-bold text-white">{&self.title}</span>
                <div tw="flex flex-1 items-end gap-4 border-b-2 border-slate-700">
                    for (_, value) in &self.bars {
                        <div tw="flex flex-col flex-1 items-center justify-end h-full gap-2">
                            <span tw="text-xl font-semibold text-muted">{*value}</span>
                            <div tw="w-full rounded-t-xl bg-linear-to-t from-indigo-600 to-sky-400"
                                 style={Style::new().height_pct(*value as f32 / max as f32 * 80.0)} />
                        </div>
                    }
                </div>
                <div tw="flex gap-4">
                    for (label, _) in &self.bars {
                        <span tw="flex-1 text-center text-xl text-muted">{label}</span>
                    }
                </div>
            </div>
        }
    }
}
```

The tallest column takes 80 % of the plot height, which leaves room for its value
label. Horizontal bars are the stat sheet (§2.3). Two series: two columns per cell
with different tints. A stacked column: a `flex-col-reverse` cell whose parts have
`height_pct` shares.

### 6.2 Heatmap

Activity per day, cooldowns per slot, seats: a grid whose cells differ only in
opacity.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.heat", version = 1, size = (900, 260), format = Png)]
pub struct Heatmap {
    pub days: Vec<f32>,   // oldest first, 0.0..=1.0 each; drawn in columns of 7
}

impl TemplateView for Heatmap {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex gap-2 w-full h-full p-6 bg-surface rounded-3xl">
                for week in self.days.chunks(7) {
                    <div tw="flex flex-col flex-1 gap-2">
                        for day in week {
                            <div tw="flex flex-1 rounded-md bg-slate-800">
                                <div tw="w-full h-full rounded-md bg-emerald-400"
                                     style={Style::new().opacity(*day)} />
                            </div>
                        }
                    </div>
                }
            </div>
        }
    }
}
```

140 cells (20 weeks) are 280 nodes and still render in a few milliseconds. Quantize the
values in the handler (five steps) so that near-equal data hits the render cache.

### 6.3 What not to chart

Pie and donut charts need arcs: use a ring gauge (one share) or a segmented bar whose
segments have `width_pct` shares (several). Line and area charts need paths: use
columns. If a real plot is required, draw it with a plotting crate and attach the
bytes with `Attachment::new(bytes, "png")`; it then needs no Template.

## 7. Banners and dividers

### 7.1 Header banner

The first child of a Container: it turns a message into a screen.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.banner", version = 1, size = (1200, 300), format = Jpeg { quality: 88 })]
pub struct Banner {
    pub kicker: String,   // "Season 4"
    pub title: String,    // "Winter Tournament"
}

impl TemplateView for Banner {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="relative flex flex-col justify-end w-full h-full p-12 bg-linear-to-br from-indigo-600 via-violet-600 to-fuchsia-500 overflow-hidden">
                <div tw="absolute -top-24 -right-16 w-96 h-96 rounded-full bg-white/10" />
                <div tw="absolute -bottom-40 right-48 w-80 h-80 rounded-full bg-white/10" />
                <span tw="text-2xl font-semibold uppercase tracking-widest text-white/70">{&self.kicker}</span>
                <span tw="text-7xl font-black text-white">{&self.title}</span>
            </div>
        }
    }
}
```

- JPEG suits gradients and photos and has no transparency: keep the root full-bleed.
  This banner is about 34 KB at quality 88.
- A banner with fixed text is the same picture for everyone: its render is a cache hit
  after the first request, and a repeated upload is skipped on edits.
- Artwork behind the text: register a `StaticImage` (`images-engine.md` §6) and put
  `<img src={ART} tw="absolute inset-0 w-full h-full object-cover" />` first, then a
  `bg-black/40` overlay, then the text.
- The title is also the heading of the Text Display below (or the alt text): an image
  title alone is invisible to search and screen readers.

### 7.2 Drawn divider

A Separator is a grey line. A 1200 × 12 image is a rule in your colours:

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.rule", version = 1, size = (1200, 12), format = Png)]
pub struct Rule {
    pub tint: u32,
}

impl TemplateView for Rule {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center gap-3 w-full h-full">
                <div tw="flex-1 h-1 rounded-full bg-slate-700" />
                <div tw="w-3 h-3 rounded-full" style={Style::new().bg_rgb(self.tint)} />
                <div tw="w-16 h-1 rounded-full" style={Style::new().bg_rgb(self.tint)} />
                <div tw="w-3 h-3 rounded-full" style={Style::new().bg_rgb(self.tint)} />
                <div tw="flex-1 h-1 rounded-full bg-slate-700" />
            </div>
        }
    }
}
```

It weighs under 1 KB and costs one component and one attachment slot. Use at most one
or two per message, and give it an alt text that says it is decoration
(`alt!("Divider")`).

## 8. Boards and tiles

The image shows the state; the moves are buttons under it (a 3 × 3 board is three
Action Rows of three buttons, or a select of free cells).

```rust
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Empty,
    X,
    O,
}

#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.ttt", version = 1, size = (512, 512), format = Png)]
pub struct Board {
    pub cells: [Cell; 9],
    pub win: Option<[u8; 3]>,   // the winning line, highlighted
}

impl TemplateView for Board {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="grid grid-cols-3 gap-3 w-full h-full p-4 bg-surface rounded-3xl">
                for (i, cell) in self.cells.iter().enumerate() {
                    <div tw={if self.win.is_some_and(|w| w.contains(&(i as u8))) {
                                 tw!("flex items-center justify-center rounded-2xl bg-emerald-500/30")
                             } else {
                                 tw!("flex items-center justify-center rounded-2xl bg-slate-800")
                             }}>
                        if *cell == Cell::X {
                            <span tw="text-9xl font-black text-sky-400">"X"</span>
                        } else if *cell == Cell::O {
                            <span tw="text-9xl font-black text-rose-400">"O"</span>
                        }
                    </div>
                }
            </div>
        }
    }
}
```

- Any grid game is the same shape: `grid grid-cols-N`, one cell node per square, the
  cell's class or tint from its state (2048 tiles: a tint per value; minesweeper: a
  number or a covered tile; connect four: `rounded-full` discs; a map: tinted tiles
  with an `absolute` marker).
- Pieces with artwork: `StaticImage`s (`<img src={KNIGHT} tw="w-full h-full" />`),
  chosen by a `match` that returns the constant.
- Label rows and columns (A to H, 1 to 8) in the image when the buttons or the select
  refer to them, so the user can map a control to a square.
- A board changes on every move, so every move is a new upload: keep it at 512 × 512
  or below and PNG (flat colours compress to a few KB).

## 9. Steps and countdowns

### 9.1 Step indicator

For a step flow (UD-4): the image shows where the user is; "Step 2 of 3: Role" stays in
the Text Display.

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.steps", version = 1, size = (900, 120), format = Png)]
pub struct Stepper {
    pub labels: Vec<String>,   // 2 to 6 short labels
    pub current: u8,           // index of the current step
}

impl TemplateView for Stepper {
    fn view(&self, _frame: Frame) -> Node {
        let current = usize::from(self.current);
        view! {
            <div tw="flex items-start w-full h-full pt-4">
                for (i, label) in self.labels.iter().enumerate() {
                    <div tw="relative flex flex-col flex-1 items-center gap-2">
                        // the connector to the next step: painted before this circle, and the
                        // next cell paints its circle over the connector's end
                        if i + 1 < self.labels.len() {
                            <div tw={if i < current { tw!("absolute top-5 left-1/2 w-full h-1 bg-brand") }
                                     else { tw!("absolute top-5 left-1/2 w-full h-1 bg-slate-700") }} />
                        }
                        <div tw={if i < current {
                                     tw!("relative flex items-center justify-center w-11 h-11 rounded-full text-xl font-bold bg-brand text-white")
                                 } else if i == current {
                                     tw!("relative flex items-center justify-center w-11 h-11 rounded-full text-xl font-bold bg-white text-slate-900 border-4 border-brand")
                                 } else {
                                     tw!("relative flex items-center justify-center w-11 h-11 rounded-full text-xl font-bold bg-slate-700 text-slate-400")
                                 }}>{i + 1}</div>
                        <span tw="text-xl text-muted">{label}</span>
                    </div>
                }
            </div>
        }
    }
}
```

It is transparent, with mid-tone greys and a saturated brand colour, so it reads on
dark and light clients. The step count of a flow is fixed, so there are only as many
different pictures as steps (per locale): every one is a cache hit after its first
render.

A **before / after** pair is two renders of one Template in a two-item gallery
(`MediaGallery::new([before, after])`), or one wider Template with both halves and an
arrow between them when they must never be cropped apart.

### 9.2 Countdown

An image is frozen at render time. For a ticking countdown write `<t:{unix}:R>` in a
Text Display: the client updates it every second without any edit. Use an image for
the moment itself (tiles) and for the share of the wait that has passed:

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.countdown", version = 1, size = (900, 240), format = Png)]
pub struct Countdown {
    pub parts: Vec<(String, String)>,   // ("02", "days"), ("14", "hours"), ("07", "minutes")
    pub elapsed: f32,                   // share of the wait that has passed, 0.0..=1.0
}

impl TemplateView for Countdown {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col justify-center gap-5 w-full h-full p-8 bg-surface rounded-3xl">
                <div tw="flex justify-center gap-5">
                    for (value, unit) in &self.parts {
                        <div tw="flex flex-col items-center justify-center w-40 h-32 rounded-2xl bg-slate-800">
                            <span tw="text-6xl font-black text-white">{value}</span>
                            <span tw="text-lg uppercase tracking-widest text-muted">{unit}</span>
                        </div>
                    }
                </div>
                <div tw="flex w-full h-3 rounded-full bg-slate-800 overflow-hidden">
                    <div tw="h-full rounded-full bg-amber-400"
                         style={Style::new().width_pct(self.elapsed * 100.0)} />
                </div>
            </div>
        }
    }
}
```

Show days, hours and minutes, never seconds: the picture is as old as the last
interaction. Round `elapsed` to whole percent so that refreshes within the same minute
hit the cache. The fixed-width tiles (`w-40`) keep the layout still when digits change
(the grammar has no `tabular-nums`).

## 10. Motion

Two kinds, which combine:

- **CSS animations** move by class: `animate-spin`, `animate-pulse`, `animate-ping`,
  `animate-bounce`, a theme animation (`animate-rise` from the template's
  `noctis-theme.css`) or an arbitrary one (`animate-[rise_1s_ease-out]`).
- **Frame-driven** views compute the state from `Frame { index, count }` (§2.5): a
  fill, a needle, a counter (`(target as f32 * progress(frame, 0.6)) as u32`), rows
  that appear one by one (draw row `i` of `n` once `frame.index >= i * frame.count / n`).

### 10.1 Loading card

```rust
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "ui.loading", version = 1, size = (600, 120), format = Png,
           animation(duration_ms = 2000, fps = 15))]
pub struct Loading {
    pub label: String,
}

impl TemplateView for Loading {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex items-center gap-6 w-full h-full px-8 bg-surface">
                <div tw="animate-spin w-14 h-14 rounded-full border-8 border-slate-700 border-t-brand" />
                <div tw="flex flex-col gap-3 flex-1">
                    <span tw="text-3xl font-semibold text-white">{&self.label}</span>
                    <div tw="animate-pulse w-2/3 h-4 rounded-full bg-slate-700" />
                </div>
            </div>
        }
    }
}
```

- The duration is a whole number of cycles of every animation in the picture
  (`animate-spin`, `animate-ping` and `animate-bounce` cycle in 1 s, `animate-pulse` in
  2 s), so the loop has no jump: 2 s here.
- A loading picture is the same for everyone per label: rendered once, then served
  from the cache.
- Use it for work that outlives one interaction (a queued job, a match being made):
  answer with the loading View, and replace it when the result exists
  (`images-ui.md` §6). For work inside one interaction the auto-defer's own
  "thinking" state is enough.

### 10.2 Rules for animated Templates

- Keep the root at `w-full h-full` on every frame (`RenderError::SizeMismatch`
  otherwise) and animate the children.
- Budget: at most 90 frames, 6 s and 1 MP per frame by default; fps at most 90 for
  WebP and APNG, 50 for GIF (`images-engine.md` §8). Over the size budget the Renderer
  lowers the fps, then the size, then the quality, then falls back to a still.
- Give animated Templates an opaque, full-bleed root. GIF transparency is one bit, and
  transparent animation edges are not something to rely on across clients.
- A still of a CSS-animated Template is its first keyframe (for `animate-rise … both`
  that is invisible): pixel snapshots of such Templates show frame 0.
- Animations loop forever unless the spec says `plays` (§2.5), and whether a client
  plays them at all, or honours the play count, is the user's client setting (client
  behaviour, not an API rule): the alt text and the Text Display must tell the whole
  story without the motion, and the held end state must be readable on its own.

## 11. Text, emoji and remote pictures inside images

- **Localized text**: format it in the handler (`Strings::of(ctx.response_locale())`)
  and pass it as a field. One render per locale is one cache entry per locale. A
  public message uses the guild locale, so everyone sees one image.
- **Long or foreign text**: `truncate`, `line-clamp-2`, fixed-width columns. Scripts
  the bundled Inter lacks (CJK, Arabic, …) need a font registered with
  `ImageConfig::font` (`images-engine.md` §6); a missing glyph draws a box.
- **Numbers**: format them in the handler too (`"9 000 XP"`, `"1.2K"`): the view only
  places strings. Integers and floats can be children directly (`{self.level}`).
- **Unicode emoji** (`"🔥 Streak"`) draw in colour only with the `image-emoji` feature;
  without it they are boxes. Credit Twemoji when you ship it (`images-engine.md` §6).
- **Discord custom emoji** in a data field (`"gg <:pepe:123456789012345678>"`) draw
  inline automatically, application emojis included (`images-emoji.md` §6); never
  write such a token in literal `view!` text.
- **Avatars and icons**: `#[asset] avatar: RemoteImage` on the Template, built in the
  handler:

  ```rust
  let user = ctx.cache().user(target).await?;           // needs USER in the Discord cache
  let avatar = user
      .as_ref()
      .map_or_else(RemoteImage::placeholder, |u| RemoteImage::avatar(u, 256));
  ```

  `RemoteImage::guild_icon(&guild, 128)`, `RemoteImage::url("https://…")?` (allow-list
  the origin first) and `RemoteImage::custom_emoji(id, animated)` work the same way.
  Request the size you draw: a 96 px circle needs `128`, not `1024`. A failed fetch
  draws the fallback picture and logs a warning; `#[asset(required)]` fails the render
  instead. Never fetch images yourself, and never draw a user-supplied URL from an
  origin you have not allow-listed.
- **Fixed artwork** (logos, frames, item icons): `StaticImage` constants compiled into
  the binary and registered with `ImageConfig::image`.
