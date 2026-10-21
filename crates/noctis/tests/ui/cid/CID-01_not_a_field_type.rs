use noctis::prelude::*;

#[derive(Button)]
#[button("rate", defaults(style = Secondary, label = "Go"))]
struct Rate {
    f: f32,
}

impl Rate {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
