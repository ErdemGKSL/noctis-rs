use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("beta stats", description = "d", flags = ["Beta"])]
struct BetaStats;

impl BetaStats {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
