use noctis::prelude::*;

#[user_command("Report")] // withdrawn form
async fn report(_ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<Responded> {
    unimplemented!()
}

#[message_command("Quote")] // withdrawn form
async fn quote(_ctx: CommandCtx, _target: noctis::ui::ResolvedMessage) -> HandlerResult<Responded> {
    unimplemented!()
}

fn main() {}
