use noctis::prelude::*;

// Story 11.1 Matrix 65: a `defaults(..)` text path is a locale key, so a
// path that is not a `MessageKey` is a type error at the path.
const LABEL: &str = "x";

#[derive(Button)]
#[button("f", defaults(style = Primary, label = LABEL))]
struct NotAKey;

impl NotAKey {
    #[handler]
    async fn handle(self, ctx: ComponentCtx) -> HandlerResult<Responded> {
        ctx.reply_text("x").await
    }
}

fn main() {}
