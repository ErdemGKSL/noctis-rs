use noctis::events::EventCtx;
use noctis::prelude::*;

#[event]
async fn h(_ctx: EventCtx, _text: String) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
