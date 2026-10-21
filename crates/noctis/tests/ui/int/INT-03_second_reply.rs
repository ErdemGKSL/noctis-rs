use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

fn msg() -> Components {
    Components::new().push(TextDisplay::new("hi"))
}

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let acked = r.reply(msg()).await?;
        Ok(acked.reply(msg()).await?.done()) // [INT-03]
    }
}

fn main() {}
