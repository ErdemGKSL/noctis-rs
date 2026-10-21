use noctis::events::{EventCtx, MessageCreate};
use noctis::prelude::*;

#[event(ordered)] // withdrawn form
async fn award_xp(_ctx: EventCtx, _msg: MessageCreate) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
