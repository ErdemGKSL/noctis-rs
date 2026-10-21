use noctis::prelude::*;

#[derive(Button)]
#[button("Shop", defaults(style = Secondary, label = "Go"))]
struct Shop;

impl Shop {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
