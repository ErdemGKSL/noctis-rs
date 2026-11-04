//! A Button and its View, defined in their own file.

use noctis::prelude::*;
use noctis::ui::{ActionRow, Components, TextDisplay, V2Message};

#[derive(Button)]
#[button("shop.next", module = "shop", defaults(style = Primary, label = "Next"))]
pub struct Next {
    pub item: u64,
}

impl Next {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.update(view(self.item + 1)).await
    }
}

pub fn view(item: u64) -> V2Message {
    V2Message::new(
        Components::new()
            .push(TextDisplay::new(format!("Item {item}")))
            .push(ActionRow::buttons([Next { item }.build()])),
    )
}
