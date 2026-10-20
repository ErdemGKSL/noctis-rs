use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[command("feedback", description = "d", auto_defer = false)]
async fn feedback(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    Ok(r.reply(Components::new().push(TextDisplay::new("hi"))).await?.done())
}

fn main() {}
