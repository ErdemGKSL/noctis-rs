//! The now-playing View: one pure [`render_player`] that `/play`, `/queue` and every
//! button call. The page is a Payload field of the Routes, the queue state comes from
//! the guild's `CallHandle` on each render, and every button updates the message in
//! place.

use noctis::prelude::*;
use noctis::rules::Limits;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;
use noctis::ui::{
    ActionRow, Budget, Button, ButtonStyle, Components, Container, ContainerComponent, TextDisplay,
    V2Message, escape_markdown,
};
use noctis::voice::{CallHandle, TrackInfo};

/// The longest track title (`Track::title` cuts longer ones).
pub const TITLE_MAX_CHARS: usize = 256;

/// The longest queue row: a row number of up to six digits plus `. ` (the `+ 8`) and a
/// title whose every character `escape_markdown` doubled. Pages are sized with it, so
/// page boundaries stay stable.
pub const ROW_MAX_CHARS: usize = 2 * TITLE_MAX_CHARS + 8;

/// A track as the View shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueuedTrack {
    /// The title (`Untitled` when the track has none).
    pub title: String,
    /// Who queued it.
    pub requested_by: Option<Id<UserMarker>>,
}

impl From<&TrackInfo> for QueuedTrack {
    fn from(track: &TrackInfo) -> Self {
        Self {
            title: track.title().unwrap_or("Untitled").to_owned(),
            requested_by: track.requested_by(),
        }
    }
}

/// What the View shows: the guild's queue, read from its call.
#[derive(Debug, Clone, PartialEq)]
pub struct PlayerState {
    /// The current track.
    pub now_playing: Option<QueuedTrack>,
    /// Whether the current track is paused.
    pub paused: bool,
    /// The queue's volume, `0.0..=2.0`.
    pub volume: f32,
    /// The pending tracks, in play order.
    pub pending: Vec<QueuedTrack>,
}

impl Default for PlayerState {
    /// Nothing playing, nothing pending, volume 1.0.
    fn default() -> Self {
        Self {
            now_playing: None,
            paused: false,
            volume: 1.0,
            pending: Vec::new(),
        }
    }
}

impl PlayerState {
    /// The state of `call`'s queue; `None` (no call) is the empty state.
    pub fn of(call: Option<&CallHandle>) -> Self {
        let Some(call) = call else {
            return Self::default();
        };
        Self {
            now_playing: call.now_playing().as_ref().map(QueuedTrack::from),
            paused: call.is_paused(),
            volume: call.volume(),
            pending: call.list().iter().map(QueuedTrack::from).collect(),
        }
    }
}

/// Pause or Resume the current track; re-renders page `page`.
#[derive(Button, Debug, Clone, Copy)]
#[button("music.toggle", defaults(style = Secondary, label = "Pause"))]
pub struct TogglePause {
    /// The queue page to show, 0-based.
    pub page: u16,
}

/// Skip the current track; re-renders page `page`.
#[derive(Button, Debug, Clone, Copy)]
#[button("music.skip", defaults(style = Secondary, label = "Skip"))]
pub struct SkipTrack {
    /// The queue page to show, 0-based.
    pub page: u16,
}

/// Stop playback and clear the queue.
#[derive(Button, Debug, Clone, Copy)]
#[button("music.stop", defaults(style = Danger, label = "Stop"))]
pub struct StopPlayback;

/// Previous and Next. The Payload is the target page, so the two ids differ (CID-02).
#[derive(Button, Debug, Clone, Copy)]
#[button("music.page", defaults(style = Secondary, label = "Next"))]
pub struct QueuePage {
    /// The page to show, 0-based.
    pub page: u16,
}

/// The player: the current track, its controls and page `page` (0-based, clamped to
/// the last page) of the queue, sized to `limits`.
pub fn render_player(state: &PlayerState, page: u16, limits: &Limits) -> V2Message {
    let Some(current) = &state.now_playing else {
        if state.pending.is_empty() {
            return frame(
                "## Music",
                vec![TextDisplay::new("-# Nothing is playing. Use /play to start.").into()],
            ); // the empty state
        }
        return frame("## Music", queue(&state.pending, page, limits));
    };
    let mut body = now_playing(current, state.paused, state.volume);
    body.push(controls(state.paused, page).into());
    body.extend(queue(&state.pending, page, limits));
    frame("## Now playing", body)
}

/// The current track's lines.
fn now_playing(track: &QueuedTrack, paused: bool, volume: f32) -> Vec<ContainerComponent> {
    let mut lines: Vec<ContainerComponent> =
        vec![TextDisplay::new(format!("**{}**", escape_markdown(&track.title))).into()];
    if let Some(user) = track.requested_by {
        lines.push(TextDisplay::new(format!("-# Requested by <@{user}>")).into());
    }
    let status = if paused { "Paused" } else { "Playing" };
    lines.push(TextDisplay::new(format!("-# {status} · Volume {}%", percent(volume))).into());
    lines
}

/// `volume` as a rounded percentage.
fn percent(volume: f32) -> String {
    format!("{:.0}", volume * 100.0)
}

/// Pause (or Resume when paused), Skip, Stop.
fn controls(paused: bool, page: u16) -> ActionRow {
    let toggle = TogglePause { page }.build();
    let toggle = if paused {
        toggle.label("Resume").style(ButtonStyle::Primary)
    } else {
        toggle
    };
    ActionRow::buttons([
        Button::from(toggle),
        Button::from(SkipTrack { page }.build()),
        Button::from(StopPlayback.build()),
    ])
}

/// How many queue rows fit on a page under `limits`: the fixed parts at their
/// largest (a 256-char title that escapes to double, the requester, the status, the
/// controls, the page line and the pager) leave room for this many longest rows.
pub fn per_page(limits: &Limits) -> usize {
    let longest = QueuedTrack {
        title: "*".repeat(TITLE_MAX_CHARS),
        requested_by: Some(Id::new(u64::MAX)),
    };
    let mut body = now_playing(&longest, true, 2.0);
    body.push(controls(true, u16::MAX).into());
    body.push(page_line(u16::MAX - 1, u16::MAX, usize::MAX).into());
    body.push(pager(0, u16::MAX).into());
    let fixed = frame("## Now playing", body);
    Budget::of(&fixed, limits)
        .fits(&TextDisplay::new("x".repeat(ROW_MAX_CHARS)))
        .max(1)
}

/// The queue part: `-# Nothing queued.`, or page `page` (clamped) of `pending`, its
/// page line and the pager.
fn queue(pending: &[QueuedTrack], page: u16, limits: &Limits) -> Vec<ContainerComponent> {
    if pending.is_empty() {
        return vec![TextDisplay::new("-# Nothing queued.").into()];
    }
    let per_page = per_page(limits);
    let pages = u16::try_from(pending.len().div_ceil(per_page)).unwrap_or(u16::MAX);
    let page = page.min(pages - 1); // the page is client input: clamp it
    let start = usize::from(page) * per_page;
    let mut rows: Vec<ContainerComponent> = pending[start..]
        .iter()
        .take(per_page)
        .zip(start + 1..)
        .map(|(track, n)| {
            TextDisplay::new(format!("{n}. {}", escape_markdown(&track.title))).into()
        })
        .collect();
    rows.push(page_line(page, pages, pending.len()).into());
    rows.push(pager(page, pages).into());
    rows
}

/// `-# Page 2 of 7 · 40 queued`.
fn page_line(page: u16, pages: u16, queued: usize) -> TextDisplay {
    TextDisplay::new(format!(
        "-# Page {} of {pages} · {queued} queued",
        u32::from(page) + 1
    ))
}

/// Previous (disabled on the first page) and Next (disabled on the last).
fn pager(page: u16, pages: u16) -> ActionRow {
    ActionRow::buttons([
        QueuePage {
            page: page.saturating_sub(1),
        }
        .build()
        .label("Previous")
        .disabled(page == 0),
        QueuePage {
            page: page.saturating_add(1),
        }
        .build()
        .disabled(u32::from(page) + 1 >= u32::from(pages)),
    ])
}

/// The title, then `body`, in one Container.
fn frame(title: &'static str, body: Vec<ContainerComponent>) -> V2Message {
    let header = Container::new().push(TextDisplay::new(title));
    V2Message::new(Components::new().push(body.into_iter().fold(header, Container::push)))
}

/// The guild's player after a button, on the button's page.
fn current(ctx: &ComponentCtx, page: u16) -> V2Message {
    let call = ctx.guild_id().and_then(|guild| ctx.voice().call(guild));
    render_player(&PlayerState::of(call.as_ref()), page, ctx.limits())
}

/// The guild's call, if the bot is in a voice channel there.
fn call_of(ctx: &ComponentCtx) -> Option<CallHandle> {
    ctx.guild_id().and_then(|guild| ctx.voice().call(guild))
}

impl TogglePause {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if let Some(call) = call_of(&ctx) {
            // A stale call (left meanwhile) changes nothing; the render shows it.
            let _changed = if call.is_paused() {
                call.resume()
            } else {
                call.pause()
            };
        }
        ctx.update(current(&ctx, self.page)).await
    }
}

impl SkipTrack {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if let Some(call) = call_of(&ctx) {
            let _skipped = call.skip();
        }
        ctx.update(current(&ctx, self.page)).await
    }
}

impl StopPlayback {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        if let Some(call) = call_of(&ctx) {
            let _stopped = call.stop();
        }
        ctx.update(current(&ctx, 0)).await
    }
}

impl QueuePage {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(current(&ctx, self.page)).await
    }
}

#[cfg(test)]
mod tests {
    use noctis::ui::validate;

    use super::*;

    fn track(title: &str) -> QueuedTrack {
        QueuedTrack {
            title: title.to_owned(),
            requested_by: Some(Id::new(40)),
        }
    }

    fn playing(pending: Vec<QueuedTrack>) -> PlayerState {
        PlayerState {
            now_playing: Some(track("a")),
            pending,
            ..PlayerState::default()
        }
    }

    /// The message as sent, as compact JSON.
    fn json(msg: &V2Message) -> String {
        noctis::testkit::render_json(msg)
            .map(|json| json.to_string())
            .unwrap_or_default()
    }

    /// The labels of the buttons, in order, with their style and disabled flag.
    fn buttons(msg: &V2Message) -> Vec<(String, u64, bool)> {
        let mut out = Vec::new();
        let mut stack = vec![noctis::testkit::render_json(msg).unwrap_or_default()];
        while let Some(value) = stack.pop() {
            match value {
                serde_json::Value::Object(map) => {
                    if map.get("type").and_then(serde_json::Value::as_u64) == Some(2) {
                        out.push((
                            map["label"].as_str().unwrap_or_default().to_owned(),
                            map["style"].as_u64().unwrap_or_default(),
                            map.get("disabled").and_then(serde_json::Value::as_bool) == Some(true),
                        ));
                    }
                    stack.extend(map.into_iter().map(|(_, v)| v).rev());
                }
                serde_json::Value::Array(items) => stack.extend(items.into_iter().rev()),
                _ => {}
            }
        }
        out
    }

    fn validates(msg: &V2Message) {
        let mut msg = msg.clone();
        let report = validate(&mut msg, &Limits::default());
        assert!(report.is_ok(), "{report:?}");
    }

    #[test]
    fn empty_state_has_no_buttons() {
        let msg = render_player(&PlayerState::default(), 0, &Limits::default());
        let text = json(&msg);
        assert!(text.contains("## Music"), "{text}");
        assert!(text.contains("-# Nothing is playing. Use /play to start."));
        assert!(buttons(&msg).is_empty());
        assert_eq!(PlayerState::of(None), PlayerState::default());
    }

    #[test]
    fn playing_without_a_queue() {
        let msg = render_player(&playing(Vec::new()), 0, &Limits::default());
        let text = json(&msg);
        for line in [
            "## Now playing",
            "**a**",
            "-# Requested by <@40>",
            "-# Playing · Volume 100%",
            "-# Nothing queued.",
        ] {
            assert!(text.contains(line), "{line}: {text}");
        }
        assert_eq!(
            buttons(&msg),
            [
                ("Pause".to_owned(), 2, false),
                ("Skip".to_owned(), 2, false),
                ("Stop".to_owned(), 4, false),
            ]
        );
        assert!(!text.contains("Page"));
        validates(&msg);
    }

    #[test]
    fn paused_shows_resume_and_volume() {
        let state = PlayerState {
            paused: true,
            volume: 0.5,
            ..playing(Vec::new())
        };
        let msg = render_player(&state, 0, &Limits::default());
        assert!(json(&msg).contains("-# Paused · Volume 50%"));
        assert_eq!(buttons(&msg)[0], ("Resume".to_owned(), 1, false));
        for (volume, shown) in [(2.0, "Volume 200%"), (0.0, "Volume 0%")] {
            let state = PlayerState {
                volume,
                ..playing(Vec::new())
            };
            assert!(json(&render_player(&state, 0, &Limits::default())).contains(shown));
        }
    }

    #[test]
    fn titles_are_escaped_and_fit() {
        let state = PlayerState {
            now_playing: Some(track("*_~")),
            ..PlayerState::default()
        };
        assert!(json(&render_player(&state, 0, &Limits::default())).contains(r"**\\*\\_\\~**"));
        let longest = "*".repeat(TITLE_MAX_CHARS);
        let state = PlayerState {
            now_playing: Some(track(&longest)),
            pending: vec![track(&longest); 50],
            paused: true,
            volume: 2.0,
        };
        for page in 0..10 {
            validates(&render_player(&state, page, &Limits::default()));
        }
    }

    #[test]
    fn a_short_queue_is_one_page() {
        let per = per_page(&Limits::default());
        assert!(per > 1, "{per}");
        for n in [1, per] {
            let pending: Vec<_> = (0..n).map(|i| track(&format!("t{i}"))).collect();
            let msg = render_player(&playing(pending), 0, &Limits::default());
            let text = json(&msg);
            assert!(text.contains("1. t0"), "{text}");
            assert!(
                text.contains(&format!("-# Page 1 of 1 · {n} queued")),
                "{text}"
            );
            let pager: Vec<_> = buttons(&msg).into_iter().skip(3).collect();
            assert_eq!(
                pager,
                [
                    ("Previous".to_owned(), 2, true),
                    ("Next".to_owned(), 2, true)
                ]
            );
            validates(&msg);
        }
    }

    /// 1000 pending tracks (the default `max_queue`): every page validates and starts
    /// at row `p * per_page + 1`; a page past the end is the last page; the two
    /// pager ids differ on every page.
    #[test]
    fn a_full_queue_pages() {
        let limits = Limits::default();
        let per = per_page(&limits);
        let pending: Vec<_> = (1..=1000).map(|i| track(&format!("t{i}"))).collect();
        let state = playing(pending);
        let pages = 1000_usize.div_ceil(per);
        for page in 0..pages {
            let msg = render_player(&state, u16::try_from(page).unwrap_or(0), &limits);
            validates(&msg);
            let text = json(&msg);
            let first = page * per + 1;
            assert!(text.contains(&format!("\"{first}. t{first}\"")), "{text}");
            let all = buttons(&msg);
            assert_eq!(all[3].2, page == 0, "Previous on {page}");
            assert_eq!(all[4].2, page + 1 == pages, "Next on {page}");
            let ids: Vec<String> = render_ids(&msg);
            let unique: std::collections::BTreeSet<_> = ids.iter().collect();
            assert_eq!(unique.len(), ids.len(), "{ids:?}");
        }
        let past = json(&render_player(&state, u16::MAX, &limits));
        assert!(past.contains(&format!("Page {pages} of {pages}")), "{past}");
        assert!(past.contains("\"1000. t1000\""));
    }

    /// Every `custom_id` of the message.
    fn render_ids(msg: &V2Message) -> Vec<String> {
        let text = noctis::testkit::render_json(msg)
            .map(|json| json.to_string())
            .unwrap_or_default();
        text.split("\"custom_id\":\"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next().map(str::to_owned))
            .collect()
    }
}
