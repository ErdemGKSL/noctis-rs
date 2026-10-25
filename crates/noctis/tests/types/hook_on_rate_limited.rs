use noctis::lifecycle::{AppStarted, HookCtx};
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};

#[derive(Hook)]
struct Up;

impl Up {
    #[handler]
    async fn handle(self, _ctx: HookCtx, _ev: AppStarted) -> HandlerResult<()> {
        Ok(())
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(V2Message::new(Components::new().push(TextDisplay::new("slow down"))))
    }
}

fn main() {}
