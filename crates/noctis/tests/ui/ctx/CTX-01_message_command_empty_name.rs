use noctis::prelude::*;

#[message_command("")]
async fn quote(_ctx: CommandCtx, _target: noctis::ui::ResolvedMessage) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
