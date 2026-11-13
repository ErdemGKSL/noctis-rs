//! `/leaderboard`: loads the board and the activity, renders both images at once and
//! answers with `render_leaderboard`.

use noctis::id::marker::{GuildMarker, UserMarker};
use noctis::image::RemoteImage;
use noctis::prelude::*;
use noctis::{DiscordCacheReader, Kv};

use super::view::{ActivityChart, BoardCard, BoardImages, BoardRow, render_leaderboard};
use super::{
    ACTIVITY_DAYS, MAX_RANKED_MEMBERS, XP_PER_LEVEL, day_key, member_count_key, member_key, today,
    xp_key,
};
use crate::strings::{Strings, keys};

/// Rows on the board: one page of the 900 × 600 card.
pub const BOARD_ROWS: usize = 5;

/// `/leaderboard`: the server's top members and its activity over the last two weeks.
#[derive(SlashCommand, Debug)]
#[command(
    "leaderboard",
    contexts = [Guild],
    rate_limit(channel, 2, per = "30s"),
    module = "rank",
)]
pub struct Leaderboard;

impl Leaderboard {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let guild = ctx
            .guild_id()
            .ok_or_else(|| HandlerError::user(keys::COMMON_GUILD_ONLY))?;
        let s = Strings::of(ctx.response_locale());
        let ranked = load_ranked(&ctx.kv(), &ctx.cache(), guild).await?;
        let images = if ranked.is_empty() {
            None
        } else {
            let days = load_days(&ctx.kv(), guild).await?;
            let (activity, board) = (activity_chart(days, &s), board_card(&ranked, &s));
            let board_alt = board_alt(&ranked, &s);
            // Both renders run at once; each is cached by its data.
            let (activity, board) = tokio::try_join!(ctx.render(activity), ctx.render(board))?;
            Some(BoardImages {
                activity,
                board,
                board_alt,
            })
        };
        ctx.respond(render_leaderboard(images, &s, ctx.emojis()))
            .await
    }
}

/// One member on the board, loaded.
#[derive(Debug, Clone)]
pub struct Ranked {
    pub name: String,
    pub xp: u64,
    pub avatar: RemoteImage,
}

/// The top [`BOARD_ROWS`] members by XP, highest first. Names and avatars come from the
/// Discord cache (USER is filled from interactions); an unseen member shows their id
/// and the placeholder avatar.
async fn load_ranked(
    kv: &Kv,
    cache: &DiscordCacheReader,
    guild: Id<GuildMarker>,
) -> HandlerResult<Vec<Ranked>> {
    let members = kv
        .counter(&member_count_key(guild))
        .await?
        .min(MAX_RANKED_MEMBERS);
    let mut scores = Vec::new();
    for slot in 1..=members {
        // A stored `0` is no member (`new_checked`); never `Id::new` on stored data.
        let stored: Option<u64> = kv.get(&member_key(guild, slot)).await?;
        if let Some(user) = stored.and_then(Id::<UserMarker>::new_checked) {
            scores.push((kv.counter(&xp_key(guild, user)).await?, user));
        }
    }
    scores.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut ranked = Vec::with_capacity(BOARD_ROWS);
    for (xp, id) in scores.into_iter().take(BOARD_ROWS) {
        let user = cache.user(id).await?;
        ranked.push(Ranked {
            name: user.as_ref().map_or_else(
                || id.to_string(),
                |u| u.global_name.clone().unwrap_or_else(|| u.name.clone()),
            ),
            xp,
            avatar: user
                .as_ref()
                .map_or_else(RemoteImage::placeholder, |u| RemoteImage::avatar(u, 128)),
        });
    }
    Ok(ranked)
}

/// Messages worth XP per day over the last [`ACTIVITY_DAYS`], oldest first.
async fn load_days(kv: &Kv, guild: Id<GuildMarker>) -> HandlerResult<Vec<u32>> {
    let today = today();
    let mut days = Vec::new();
    for day in (0..ACTIVITY_DAYS)
        .rev()
        .map(|ago| today.saturating_sub(ago))
    {
        let count = kv.counter(&day_key(guild, day)).await?;
        days.push(u32::try_from(count).unwrap_or(u32::MAX));
    }
    Ok(days)
}

/// The activity chart's data. Pure, so tests build it without a store.
pub fn activity_chart(days: Vec<u32>, s: &Strings) -> ActivityChart {
    ActivityChart {
        title: s.rank.activity_title().to_owned(),
        today_label: s.rank.today(days.last().copied().unwrap_or(0)),
        days,
    }
}

/// The board's data: ranks, labels and each row's share of the leader's XP.
pub fn board_card(ranked: &[Ranked], s: &Strings) -> BoardCard {
    let leader = ranked.first().map_or(1, |r| r.xp.max(1));
    BoardCard {
        title: s.rank.board_title().to_owned(),
        rows: ranked
            .iter()
            .zip(1..)
            .map(|(r, rank)| BoardRow {
                rank,
                name: r.name.clone(),
                score_label: s.rank.score(r.xp / XP_PER_LEVEL, r.xp),
                share: share(r.xp, leader),
                avatar: r.avatar.clone(),
            })
            .collect(),
    }
}

/// `xp` of `leader` as 0.0..=1.0.
#[expect(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    reason = "a display ratio in 0..=1"
)]
fn share(xp: u64, leader: u64) -> f32 {
    (xp as f64 / leader.max(1) as f64) as f32
}

/// The board's alt text: the entries in words, so the image is not the only source.
fn board_alt(ranked: &[Ranked], s: &Strings) -> String {
    let entries: Vec<String> = ranked
        .iter()
        .zip(1..)
        .map(|(r, rank)| format!("{rank}. {} {} XP", r.name, r.xp))
        .collect();
    s.rank.board_alt(entries.join(", "))
}

#[cfg(test)]
mod tests {
    use noctis::rules::Locale;

    use super::*;

    #[test]
    fn rows_share_the_leaders_xp() {
        let s = Strings::of(Locale::EnUs);
        let ranked = [("Ada", 900), ("Bo", 450)].map(|(name, xp)| Ranked {
            name: name.to_owned(),
            xp,
            avatar: RemoteImage::placeholder(),
        });
        let card = board_card(&ranked, &s);
        assert_eq!(card.rows[0].rank, 1);
        assert!((card.rows[0].share - 1.0).abs() < f32::EPSILON);
        assert!((card.rows[1].share - 0.5).abs() < f32::EPSILON);
        assert_eq!(card.rows[1].score_label, "Level 4 · 450 XP");
        assert_eq!(
            board_alt(&ranked, &s),
            "Top members: 1. Ada 900 XP, 2. Bo 450 XP"
        );
    }
}
