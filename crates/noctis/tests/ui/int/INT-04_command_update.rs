use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[derive(SlashCommand)]
#[command("ping", description = "d")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        let msg = Components::new().push(TextDisplay::new("hi"));
        Ok(r.update(msg).await?.done()) // [INT-04]
    }
}

fn main() {}
