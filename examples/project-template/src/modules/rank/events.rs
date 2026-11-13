//! Gateway events of the rank feature (tescord's "trigger" file).

use std::time::Duration;

use noctis::events::MessageCreate;
use noctis::image::RemoteImage;
use noctis::prelude::*;

use super::view::{LevelUpCard, render_level_up};
use super::{ACTIVITY_DAYS, day_key, level_reached, member_count_key, member_key, today, xp_key};
use crate::strings::Strings;

/// +1 XP per message, at most once a minute per member; on each new level it posts an
/// animated level-up card in the channel. A `MessageCreate` handler derives the
/// `GUILD_MESSAGES` intent: `build()` adds it to the App (not privileged, so no
/// `.intents(..)` call; message content and DMs are not requested) and the gateway
/// starts parsing `MESSAGE_CREATE`. Concurrent (not `ordered`), never droppable.
#[derive(EventHandler, Debug)]
#[event(module = "rank")]
pub struct AwardXp;

impl AwardXp {
    #[handler]
    async fn handle(self, ctx: EventCtx, msg: MessageCreate) -> HandlerResult<()> {
        let Some(guild) = ctx.guild_id() else {
            return Ok(());
        };
        if msg.author.bot {
            return Ok(());
        }
        let kv = ctx.kv();
        let cooldown = format!("xpcd:{guild}:{}", msg.author.id);
        if !kv
            .put_if_absent(&cooldown, &(), Some(Duration::from_mins(1)))
            .await?
        {
            return Ok(());
        }
        let xp = kv.incr(&xp_key(guild, msg.author.id), 1, None).await?;
        if xp == 1 {
            // A member's first XP: give them a leaderboard slot. `incr` is atomic, so two
            // members never share a slot (with a shared store, across processes too). Ids
            // are stored as `u64`: twilight's `Id` does not decode from the Kv's bytes.
            let slot = kv.incr(&member_count_key(guild), 1, None).await?;
            kv.put(&member_key(guild, slot), &msg.author.id.get(), None)
                .await?;
        }
        let keep = Duration::from_hours(24 * (ACTIVITY_DAYS + 1));
        kv.incr(&day_key(guild, today()), 1, Some(keep)).await?;

        let Some(level) = level_reached(xp) else {
            return Ok(());
        };
        // An event has no interaction locale: the server's preferred locale picks the
        // strings. An event handler renders like every interaction context; the card is
        // an animated WebP that plays once and rests on its last frame.
        let s = Strings::of(ctx.guild_locale());
        let name = msg
            .author
            .global_name
            .clone()
            .unwrap_or_else(|| msg.author.name.clone());
        let card = LevelUpCard {
            headline: s.rank.level_up().to_owned(),
            name: name.clone(),
            level_label: s.profile.card.level(level),
            avatar: RemoteImage::avatar(&msg.author, 256),
        };
        let card = ctx.render(card).await?;
        let view = render_level_up(&name, level, card, &s, ctx.emojis());
        // A normal bot message (Components V2); the bot may lack Send Messages in this
        // channel, which is not worth an error report.
        if let Err(error) = ctx.messages().send(msg.channel_id, view).await {
            tracing::debug!(%guild, %error, "level-up card was not posted");
        }
        Ok(())
    }
}
