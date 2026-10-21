use noctis::prelude::*;

#[derive(Button)]
#[button("a", one_shot, defaults(style = Secondary, label = "Go"))]
struct Once;

impl Once {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
