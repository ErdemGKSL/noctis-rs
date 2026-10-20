use noctis::prelude::*;

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
