use noctis::prelude::*;

#[user_command("Show profile", description = "Shows a profile")] // [CTX-02]
async fn show(_ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
