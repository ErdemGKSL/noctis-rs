use noctis::prelude::*;

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    Ok(r.show_modal(()).await?.done()) // [INT-04]
}

fn main() {}
