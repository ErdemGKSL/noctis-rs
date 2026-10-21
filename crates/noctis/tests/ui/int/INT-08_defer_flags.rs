use noctis::prelude::*;
use noctis::ui::CallbackFlags;

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        Ok(r.defer_reply_as(CallbackFlags::EPHEMERAL).await?.done()) // [INT-08]
    }
}

fn main() {}
