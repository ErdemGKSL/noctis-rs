use noctis::prelude::*;

#[user_command("Show profile", integration_types = [UserInstall], guilds = [7, 8])]
async fn show(_ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
