use noctis::events::EventCtx;
use noctis::prelude::*;
use noctis::twilight::model::gateway::payload::incoming::InteractionCreate;

#[event]
async fn h(_ctx: EventCtx, _interaction: InteractionCreate) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
