use noctis::prelude::*;

#[user_command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
async fn show(_ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
