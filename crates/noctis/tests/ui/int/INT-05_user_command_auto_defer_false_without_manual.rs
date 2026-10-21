use noctis::prelude::*;
use noctis::ui::{Components, ResolvedUser, TextDisplay};

#[derive(UserCommand)]
#[user_command("Report", auto_defer = false)]
struct Report;

impl Report {
    #[handler]
    async fn handle(self, _ctx: CommandCtx,
    _target: ResolvedUser,
    r: CommandResponder,) -> HandlerResult<Responded> {
        Ok(r.reply(Components::new().push(TextDisplay::new("hi"))).await?.done())
    }
}

fn main() {}
