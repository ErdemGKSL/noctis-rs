use noctis::prelude::*;

#[command("it's", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
