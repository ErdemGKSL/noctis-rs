use noctis::prelude::*;

#[command("ping", description = "d")] // withdrawn form
async fn ping(_ctx: CommandCtx) -> HandlerResult<Responded> {
    unimplemented!()
}

fn main() {}
