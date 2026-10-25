use noctis::events::MessageCreate;
use noctis::lifecycle::HookCtx;
use noctis::prelude::*;

#[derive(Hook)]
struct Up;

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: MessageCreate) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
