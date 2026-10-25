use noctis::lifecycle::HookCtx;
use noctis::prelude::*;

#[derive(Hook)]
struct Up;

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
