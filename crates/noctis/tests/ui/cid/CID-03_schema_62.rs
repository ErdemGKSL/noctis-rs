use noctis::prelude::*;

#[derive(Button)]
#[button("a", schema = 62, defaults(style = Secondary, label = "Go"))]
struct Schema;

impl Schema {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
