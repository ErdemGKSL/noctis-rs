use noctis::prelude::*;

#[derive(Button)]
#[button("_internal", defaults(style = Secondary, label = "Go"))]
struct Internal;

impl Internal {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
