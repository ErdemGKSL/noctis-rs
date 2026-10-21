use noctis::prelude::*;

#[derive(Button)]
#[button("a", by_ref, signed, defaults(style = Secondary, label = "Go"))]
struct RefSigned {
    s: String,
}

impl RefSigned {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
