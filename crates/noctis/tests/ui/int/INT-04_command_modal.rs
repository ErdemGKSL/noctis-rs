use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        Ok(r.show_modal(()).await?.done()) // [INT-04]
    }
}

fn main() {}
