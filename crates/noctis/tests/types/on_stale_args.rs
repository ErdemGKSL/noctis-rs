use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};

fn view() -> V2Message {
    V2Message::new(Components::new().push(TextDisplay::new("hi")))
}

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<V2Message> {
        Ok(view())
    }

    #[on_stale(response = ephemeral)]
    async fn stale(_ctx: ComponentCtx) -> HandlerResult<V2Message> {
        Ok(view())
    }
}

fn main() {}
