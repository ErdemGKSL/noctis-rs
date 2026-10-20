use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[command("feedback", description = "d", auto_defer = true)]
async fn feedback(_ctx: CommandCtx, r: CommandResponder<Manual>) -> HandlerResult<Responded> {
    Ok(r.reply(Components::new().push(TextDisplay::new("hi"))).await?.done())
}

fn main() {}
