use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("daily", description = "d", rate_limit(message, 1, per = "1m"))]
struct Daily;

impl Daily {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
