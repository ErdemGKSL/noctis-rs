use noctis::events::EventCtx;
use noctis::prelude::*;

#[derive(EventHandler)]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _text: String) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
