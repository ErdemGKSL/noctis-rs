use noctis::events::EventCtx;
use noctis::prelude::*;
use noctis::twilight::model::gateway::payload::incoming::InteractionCreate;

#[derive(EventHandler)]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _interaction: InteractionCreate) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
