use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
