//! An event handler and a Lifecycle Hook, defined in their own file.

use noctis::events::{EventCtx, MessageCreate};
use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;

#[derive(EventHandler)]
#[event(module = "xp")]
pub struct AwardXp;

impl AwardXp {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _event: MessageCreate) -> HandlerResult<()> {
        Ok(())
    }
}

#[derive(Hook)]
#[hook(module = "xp")]
pub struct Started;

impl Started {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _event: AppStarted) -> HandlerResult<()> {
        Ok(())
    }
}
