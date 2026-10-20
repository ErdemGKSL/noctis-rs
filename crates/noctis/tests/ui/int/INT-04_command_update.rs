use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay};

#[command("ping", description = "d")]
async fn ping(_ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
    let msg = Components::new().push(TextDisplay::new("hi"));
    Ok(r.update(msg).await?.done()) // [INT-04]
}

fn main() {}
