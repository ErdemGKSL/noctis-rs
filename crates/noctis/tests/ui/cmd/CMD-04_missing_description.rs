use noctis::prelude::*;

/// Doc comments never become the description.
#[command("ping")]
async fn h(_ctx: CommandCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
