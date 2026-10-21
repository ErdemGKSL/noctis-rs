use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[derive(SlashCommand)]
#[command("feedback", description = "d", auto_defer = false)]
struct Feedback;

impl Feedback {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        Ok(r.reply(Components::new().push(TextDisplay::new("hi"))).await?.done())
    }
}

fn main() {}
