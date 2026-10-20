use noctis::prelude::*;

#[command("feedback", description = "d", auto_defer = false)]
async fn feedback(_ctx: CommandCtx) -> HandlerResult<noctis::ui::Components> {
    Ok(noctis::ui::Components::new())
}

fn main() {}
