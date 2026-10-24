use noctis::prelude::*;

// Story 12.5: `ctx.cache()` returns owned values, never a dashmap guard
// (twilight's in-memory cache hands out guards with `.value()`), so no
// guard can be held across an `.await`.
#[derive(SlashCommand)]
#[command("guild", description = "d")]
struct GuildName;

impl GuildName {
    #[handler]
    async fn handle(self, ctx: CommandCtx) -> HandlerResult<Responded> {
        let id = ctx.guild_id().unwrap();
        let g = ctx.cache().guild(id).await?.unwrap();
        let _ = g.value();
        unimplemented!()
    }
}

fn main() {}
