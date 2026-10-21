use noctis::prelude::*;

#[derive(Button)]
#[button("a", owner = user, defaults(style = Secondary, label = "Go"))]
struct Owned;

impl Owned {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
