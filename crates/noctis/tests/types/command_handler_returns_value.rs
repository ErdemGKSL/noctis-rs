use noctis::prelude::*;

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx) -> HandlerResult<u8> {
    Ok(1)
}

fn main() {}
