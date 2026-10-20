use noctis::prelude::*;
use noctis::ui::{Components, ResolvedUser, TextDisplay};

#[user_command("Report", auto_defer = false)]
async fn report(
    _ctx: CommandCtx,
    _target: ResolvedUser,
    r: CommandResponder,
) -> HandlerResult<Responded> {
    Ok(r.reply(Components::new().push(TextDisplay::new("hi"))).await?.done())
}

fn main() {}
