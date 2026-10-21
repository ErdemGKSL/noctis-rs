use noctis::prelude::*;

#[derive(Button)]
#[button("a", by_ref, ttl = "15x", defaults(style = Secondary, label = "Go"))]
struct Typo;

impl Typo {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
