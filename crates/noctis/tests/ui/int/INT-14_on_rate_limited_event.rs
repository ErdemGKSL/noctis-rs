use noctis::events::{EventCtx, MessageCreate};
use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};

#[derive(EventHandler)]
struct Greeter;

impl Greeter {
    #[handler]
    async fn handle(self, _ctx: EventCtx, _msg: MessageCreate) -> HandlerResult<()> {
        Ok(())
    }

    #[on_rate_limited]
    async fn limited(_ctx: RateLimitedCtx, _hit: RateLimitHit) -> HandlerResult<V2Message> {
        Ok(V2Message::new(Components::new().push(TextDisplay::new("slow"))))
    }
}

fn main() {}
