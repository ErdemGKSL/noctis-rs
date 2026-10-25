use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;

#[derive(Hook)]
struct Up {
    count: u8,
}

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
        let _ = self.count;
        Ok(())
    }
}

fn main() {}
