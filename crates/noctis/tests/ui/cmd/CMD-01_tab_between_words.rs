use noctis::prelude::*;

#[command("config\tset", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
