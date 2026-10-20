use noctis::prelude::*;

#[command("ping", description = "d", response = update)] // [INT-04]
async fn ping(_ctx: CommandCtx) -> HandlerResult<noctis::ui::Components> {
    Ok(noctis::ui::Components::new())
}

fn main() {}
