// NOTE: written against the planned Noctis API (architecture spine); updated as crates land.

//! Gateway events of the profile feature (tescord's "trigger" file).

use std::time::Duration;

use noctis::events::MessageCreate;
use noctis::prelude::*;

use super::xp_key;

/// +1 XP per message, at most once a minute per member. A `MessageCreate` handler
/// derives the GUILD_MESSAGES intent: `build()` adds it to the App (not privileged, so
/// no `.intents(..)` call; message content and DMs are not requested) and the gateway
/// starts parsing MESSAGE_CREATE. Concurrent (not `ordered`), never droppable.
/// [planned: Story 6.11] a unit struct with `#[derive(EventHandler)]` (shipped as a free
/// `#[event]` function).
#[derive(EventHandler, Debug)]
#[event]
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
        let kv = ctx.kv(); // [planned, Epic 7]
        let cooldown = format!("xpcd:{guild}:{}", msg.author.id);
        if kv.put_if_absent(&cooldown, &(), Some(Duration::from_secs(60))).await? {
            kv.incr(&xp_key(guild, msg.author.id), 1, None).await?;
        }
        Ok(())
    }
}
