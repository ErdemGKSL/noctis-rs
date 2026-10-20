use noctis::prelude::*;

#[command("ping", description = "d", dm_permission = false)] // [META-05]
async fn ping(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
