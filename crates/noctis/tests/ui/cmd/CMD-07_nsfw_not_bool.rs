use noctis::prelude::*;

#[command("ping", description = "d", nsfw = 1)]
async fn ping(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
