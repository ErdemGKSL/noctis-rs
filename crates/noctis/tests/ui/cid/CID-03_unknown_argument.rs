use noctis::prelude::*;

#[derive(Button)]
#[button("a", colour, defaults(style = Secondary, label = "Go"))]
struct Colour;

impl Colour {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
