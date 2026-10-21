use noctis::prelude::*;

#[derive(Button)]
#[button("a", view, defaults(style = Secondary, label = "Go"))]
struct Tabs;

impl Tabs {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
