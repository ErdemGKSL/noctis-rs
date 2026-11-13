//! The rank feature's Templates (images) and the render functions of its two messages.
//! Every image is read-only: actions live in components, never in a picture.
//!
//! - [`LevelUpCard`]: an animated card that plays once and rests on its last frame: a
//!   ring gauge (`shape::Arc`) fills around the avatar.
//! - [`ActivityChart`]: messages per day as an area under a line (`shape::Area`,
//!   `shape::Line`), today's value marked with a dot placed by `left_pct`/`bottom_pct`.
//! - [`BoardCard`]: the top members, each row with its own avatar: a nested
//!   `#[derive(Assets)]` row type.

use noctis::emoji::Emojis;
use noctis::image::shape::{Arc, Area, Line};
use noctis::image::{Assets, Frame, Node, RemoteImage, Style, Template, TemplateView, tw, view};
use noctis::prelude::*;
use noctis::ui::{
    Attachment, AttachmentSet, Components, Container, MediaGallery, MediaItem, TextDisplay,
    V2Message, escape_markdown,
};
use serde::{Deserialize, Serialize};

use crate::emoji::Icons;
use crate::strings::Strings;

/// `--color-brand` of `noctis-theme.css`, for shapes and accents (shapes take `0xRRGGBB`).
pub const BRAND: u32 = 0x58_65F2;
/// The ring's and the plot's empty track.
const TRACK: u32 = 0x1E_293B;
/// The Container accent of both messages (the same brand colour).
const ACCENT: Rgb = rgb!(0x58_65F2);

/// 0.0 → 1.0 over the frames, eased out (cubic). [`Frame::STILL`] (`index 0, count 1`)
/// is 1.0, so a still render and the held last frame show the end state.
#[expect(clippy::cast_precision_loss, reason = "frame counts are small")]
fn eased(frame: Frame) -> f32 {
    let t = (frame.index + 1) as f32 / frame.count.max(1) as f32;
    1.0 - (1.0 - t.min(1.0)).powi(3)
}

// ---------------------------------------------------------------------------------------
// Level-up card (posted by the `AwardXp` event handler)
// ---------------------------------------------------------------------------------------

/// The level-up card: animated WebP (the default animated format, no extra feature),
/// 1.2 s at 15 fps = 18 frames, played once (`plays = 1`), then the last frame held for
/// 4 s (`hold_end_ms`; a hold draws no frame). Whether a client honours the play count
/// is client behaviour, so the last frame reads on its own.
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(
    id = "rank.levelup",
    version = 1,
    size = (720, 240),
    format = Png,
    animation(duration_ms = 1200, fps = 15, plays = 1, hold_end_ms = 4000),
    module = "rank"
)]
pub struct LevelUpCard {
    pub headline: String,
    pub name: String,
    pub level_label: String,
    #[asset]
    pub avatar: RemoteImage,
}

impl TemplateView for LevelUpCard {
    #[expect(clippy::cast_possible_truncation, reason = "0..=16 px")]
    fn view(&self, frame: Frame) -> Node {
        let t = eased(frame);
        let rise = ((1.0 - t) * 16.0).round() as i32;
        // A ring gauge: the value's share of the circle over a track, round caps, its box
        // sized by its parent (`absolute inset-0` in a `size-48` box).
        let ring = Arc::new(t, BRAND)
            .track(TRACK)
            .thickness_pct(12.0)
            .round_caps(true)
            .classes(tw!("absolute inset-0"));
        view! {
            <div tw="flex items-center gap-10 w-full h-full px-12 bg-surface rounded-3xl">
                <div tw="relative flex items-center justify-center size-48 shrink-0">
                    {ring}
                    <img src={&self.avatar} tw="w-36 h-36 rounded-full" />
                </div>
                // The text fades in and slides into place with the ring.
                <div
                    tw="flex flex-col gap-2 flex-1"
                    style={Style::new().opacity(t).translate_px(0, rise)}
                >
                    <span tw="text-2xl font-bold tracking-widest text-brand-soft">{&self.headline}</span>
                    <span tw="text-5xl font-bold text-white truncate">{&self.name}</span>
                    <span tw="text-3xl text-muted tabular-nums">{&self.level_label}</span>
                </div>
            </div>
        }
    }
}

/// The level-up message: the rendered `card` and one line of text with the drawn
/// `level_up` application emoji (`#[emoji(render)]`; nothing while it is missing).
pub fn render_level_up(
    name: &str,
    level: u64,
    card: Attachment,
    s: &Strings,
    emojis: &Emojis,
) -> V2Message {
    let mut files = AttachmentSet::new();
    let card = files.add(card);
    // The name is user text: escape it in Markdown, never in alt text or the emoji token.
    let line = format!(
        "{} {}",
        emojis.text(Icons::LevelUp),
        s.rank.reached(escape_markdown(name), level)
    );
    let container = Container::new()
        .accent(ACCENT)
        .push(TextDisplay::new(line.trim_start().to_owned()))
        .push(MediaGallery::new([
            MediaItem::new(card).alt(s.rank.level_up_alt(name, level))
        ]));
    V2Message::new(Components::new().push(container)).attachments(files)
}

// ---------------------------------------------------------------------------------------
// Activity chart and leaderboard (`/leaderboard`)
// ---------------------------------------------------------------------------------------

/// Messages worth XP per day, oldest first, as an area under a line.
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "rank.activity", version = 1, size = (900, 300), format = Png, module = "rank")]
pub struct ActivityChart {
    pub title: String,
    /// "Today: 42", formatted by the handler.
    pub today_label: String,
    /// One count per day, oldest first; today is the last.
    pub days: Vec<u32>,
}

impl ActivityChart {
    /// `(x, y)` in `0..=1` of the plot (y up), scaled so the busiest day sits just below
    /// the top edge and the line's stroke is not clipped there.
    #[expect(clippy::cast_precision_loss, reason = "a handful of days")]
    fn points(&self) -> Vec<(f64, f64)> {
        let max = f64::from(self.days.iter().copied().max().unwrap_or(0).max(1));
        let last = self.days.len().saturating_sub(1).max(1) as f64;
        self.days
            .iter()
            .enumerate()
            .map(|(i, v)| (i as f64 / last, f64::from(*v) / max * 0.9))
            .collect()
    }
}

impl TemplateView for ActivityChart {
    #[expect(clippy::cast_possible_truncation, reason = "a ratio in 0..=1")]
    fn view(&self, _frame: Frame) -> Node {
        let points = self.points();
        let (x, y) = points.last().copied().unwrap_or((1.0, 0.0));
        view! {
            <div tw="flex flex-col gap-4 w-full h-full p-8 bg-surface rounded-3xl">
                <div tw="flex justify-between items-baseline">
                    <span tw="text-3xl font-bold text-white">{&self.title}</span>
                    <span tw="text-2xl text-muted tabular-nums">{&self.today_label}</span>
                </div>
                <div tw="relative flex flex-1 border-b-2 border-slate-700">
                    for i in [1.0_f32, 2.0, 3.0] {
                        <div tw="absolute left-0 right-0 h-px bg-slate-800"
                             style={Style::new().top_pct(i * 25.0)} />
                    }
                    // Painted in tree order: the area, then the line, then the marker.
                    {Area::new(points.iter().copied(), 0x23_2A6B).classes(tw!("absolute inset-0"))}
                    {Line::new(points.iter().copied(), BRAND).stroke_px(4.0).classes(tw!("absolute inset-0"))}
                    // Today's value: a 20 px dot whose centre sits on the last point
                    // (half its size back to the left, half down).
                    <div tw="absolute size-5 rounded-full bg-white border-4 border-brand"
                         style={Style::new()
                             .left_pct(x as f32 * 100.0)
                             .bottom_pct(y as f32 * 100.0)
                             .translate_px(-10, 10)} />
                </div>
            </div>
        }
    }
}

/// One leaderboard row. `#[derive(Assets)]` declares its `#[asset]` field to the
/// Template that holds the rows: each row's avatar is fetched before the render, and an
/// error names the row (`rows[2].avatar`).
#[derive(Assets, Serialize, Deserialize, Debug, Clone)]
pub struct BoardRow {
    pub rank: u32,
    pub name: String,
    /// "Level 7 · 742 XP", formatted by the handler.
    pub score_label: String,
    /// This row's XP as a share of the leader's, 0.0..=1.0.
    pub share: f32,
    #[asset]
    pub avatar: RemoteImage,
}

/// The top members (at most [`super::handlers::BOARD_ROWS`]): one page.
#[derive(Template, Serialize, Deserialize, Debug)]
#[template(id = "rank.board", version = 1, size = (900, 600), format = Png, module = "rank")]
pub struct BoardCard {
    pub title: String,
    /// `#[asset]` on a field holding `Assets` rows: their own assets are declared.
    #[asset]
    pub rows: Vec<BoardRow>,
}

/// The medal colour of a rank.
fn medal(rank: u32) -> u32 {
    match rank {
        1 => 0xFB_BF24,
        2 => 0xCB_D5E1,
        3 => 0xD9_7706,
        _ => 0x33_4155,
    }
}

impl TemplateView for BoardCard {
    fn view(&self, _frame: Frame) -> Node {
        view! {
            <div tw="flex flex-col gap-3 w-full h-full p-8 bg-surface rounded-3xl">
                <span tw="text-4xl font-bold text-white">{&self.title}</span>
                for row in &self.rows {
                    <div tw="relative flex items-center gap-5 h-20 px-5 rounded-2xl bg-slate-900 overflow-hidden">
                        // The share as the row's own background: painted first, so under the rest.
                        <div tw="absolute left-0 top-0 h-full bg-brand/20"
                             style={Style::new().width_pct(row.share * 100.0)} />
                        <div tw="flex items-center justify-center w-12 h-12 rounded-full text-2xl font-black text-slate-900"
                             style={Style::new().bg_rgb(medal(row.rank))}>{row.rank}</div>
                        <img src={&row.avatar} tw="w-14 h-14 rounded-full" />
                        <span tw="flex-1 text-3xl font-semibold text-white truncate">{&row.name}</span>
                        <span tw="text-2xl font-bold text-brand-soft tabular-nums">{&row.score_label}</span>
                    </div>
                }
            </div>
        }
    }
}

/// The two rendered images of a non-empty leaderboard.
pub struct BoardImages {
    pub activity: Attachment,
    pub board: Attachment,
    /// The board's alt text: the top entries in words.
    pub board_alt: String,
}

/// The `/leaderboard` message: both images in one Media Gallery, or the designed empty
/// state while nobody has XP yet.
pub fn render_leaderboard(images: Option<BoardImages>, s: &Strings, emojis: &Emojis) -> V2Message {
    // `text` writes nothing while the emoji is missing: keep the heading well-formed.
    let icon = emojis.text(Icons::LevelUp);
    let title = if icon.is_missing() {
        format!("## {}", s.rank.board_title())
    } else {
        format!("## {icon} {}", s.rank.board_title())
    };
    let container = Container::new()
        .accent(ACCENT)
        .push(TextDisplay::new(title));
    let Some(images) = images else {
        let container = container.push(TextDisplay::new(format!("-# {}", s.rank.board_empty())));
        return V2Message::new(Components::new().push(container));
    };
    let mut files = AttachmentSet::new();
    let activity = files.add(images.activity);
    let board = files.add(images.board);
    let container = container.push(MediaGallery::new([
        MediaItem::new(activity).alt(s.rank.activity_alt()),
        MediaItem::new(board).alt(images.board_alt),
    ]));
    V2Message::new(Components::new().push(container)).attachments(files)
}
