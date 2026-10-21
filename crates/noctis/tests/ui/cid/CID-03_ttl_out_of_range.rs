use noctis::prelude::*;

#[derive(Button)]
#[button("a", by_ref, ttl = "366d", defaults(style = Secondary, label = "Go"))]
struct Forever;

impl Forever {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
