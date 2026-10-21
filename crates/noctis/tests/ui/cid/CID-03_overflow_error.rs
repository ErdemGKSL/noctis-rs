use noctis::prelude::*;

#[derive(Button)]
#[button("a", overflow = error, defaults(style = Secondary, label = "Go"))]
struct Overflow;

impl Overflow {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
