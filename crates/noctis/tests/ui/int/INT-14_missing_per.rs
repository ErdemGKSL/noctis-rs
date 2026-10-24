use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("daily", description = "d", rate_limit(user, 3))]
struct Daily;

impl Daily {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
