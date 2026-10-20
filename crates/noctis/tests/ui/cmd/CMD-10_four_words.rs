use noctis::prelude::*;

#[command("a b c d", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
