use noctis::prelude::*;

#[command("config  set", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
