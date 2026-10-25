use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;

struct User;

#[derive(Hook)]
struct Up;

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: AppStarted, _user: Ext<User>) -> HandlerResult<()> {
        Ok(())
    }
}

fn main() {}
