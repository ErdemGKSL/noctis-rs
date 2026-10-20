use noctis::prelude::*;

#[command("feedback", description = "d", opens_modal)]
async fn feedback(_ctx: CommandCtx) -> HandlerResult<noctis::ui::Components> {
    Ok(noctis::ui::Components::new())
}

fn main() {}
