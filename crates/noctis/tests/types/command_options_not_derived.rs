use noctis::prelude::*;

#[command("warn", description = "d")]
async fn warn(_ctx: CommandCtx, _options: String) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
