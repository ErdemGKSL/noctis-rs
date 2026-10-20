use noctis::prelude::*;

#[command("config (set|get) (prefix|welcome)?", description = "d")]
async fn h(_ctx: CommandCtx, _path: HPath) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
