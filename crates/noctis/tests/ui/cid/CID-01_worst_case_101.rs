use noctis::codec::BoundedStr;
use noctis::prelude::*;

#[derive(Button)]
#[button("a", defaults(style = Secondary, label = "Go"))]
struct TooLong {
    t: BoundedStr<47>,
    n: u16,
}

impl TooLong {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
