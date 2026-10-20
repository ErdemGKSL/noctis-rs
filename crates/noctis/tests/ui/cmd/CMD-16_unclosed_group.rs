use noctis::prelude::*;

#[command("a (b", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
