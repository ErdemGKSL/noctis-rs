use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("daily", description = "d", rate_limit(user, 0, per = "10s"))]
struct Daily;

impl Daily {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
