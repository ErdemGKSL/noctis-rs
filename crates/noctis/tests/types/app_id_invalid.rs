use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("admin", description = "d", apps = ["Main"])]
struct Admin;

impl Admin {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
