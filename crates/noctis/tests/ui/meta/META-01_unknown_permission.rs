use noctis::prelude::*;

#[command("config", description = "d", permissions = MANAGE_SERVER)]
async fn config(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
