use noctis::prelude::*;

// Story 6.11 Matrix 8: one id, one handler.
#[derive(Button)]
#[button("a", defaults(style = Primary, label = "A"))]
struct Twice;

impl Twice {
    #[handler]
    async fn first(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }

    #[handler]
    async fn second(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
