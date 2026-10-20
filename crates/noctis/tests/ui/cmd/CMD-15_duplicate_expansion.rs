use noctis::prelude::*;

#[command("a (b|b)", description = "d")]
async fn h(_ctx: CommandCtx, _path: HPath) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
