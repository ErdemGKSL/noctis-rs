use noctis::prelude::*;

#[command("ping", description = "d", guilds = [42], contexts = [Guild])]
async fn ping(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
