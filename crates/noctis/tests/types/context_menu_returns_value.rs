use noctis::prelude::*;

#[user_command("Show profile")]
async fn show(_ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<u8> {
    Ok(1)
}

fn main() {}
