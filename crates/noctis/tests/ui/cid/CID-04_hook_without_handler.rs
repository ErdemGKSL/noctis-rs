use noctis::prelude::*;
use noctis::ui::{Components, TextDisplay, V2Message};

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

// A hook alone is not a handler: only the CID-04 error.
impl Buy {
    #[on_stale]
    async fn stale(_ctx: ComponentCtx) -> HandlerResult<V2Message> {
        Ok(V2Message::new(Components::new().push(TextDisplay::new("hi"))))
    }
}

fn main() {}
