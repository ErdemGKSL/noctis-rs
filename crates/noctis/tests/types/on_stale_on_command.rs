use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};

fn view() -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new("hi")))
}

#[derive(SlashCommand)]
#[command("ping", description = "Ping")]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<V2Message> {
        Ok(view())
    }

    #[on_stale]
    async fn stale(_ctx: CommandCtx) -> HandlerResult<V2Message> {
        Ok(view())
    }
}

fn main() {}
