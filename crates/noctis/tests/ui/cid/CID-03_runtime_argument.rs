use noctis::prelude::*;

#[derive(Button)]
#[button("a", ttl = "1m", defaults(style = Secondary, label = "Go"))]
struct Timed;

impl Timed {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
