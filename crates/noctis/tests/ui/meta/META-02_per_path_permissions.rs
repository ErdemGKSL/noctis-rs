use noctis::prelude::*;

#[command("config (set|get)", description = "d", permissions(set = MANAGE_GUILD))]
async fn config(_ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
