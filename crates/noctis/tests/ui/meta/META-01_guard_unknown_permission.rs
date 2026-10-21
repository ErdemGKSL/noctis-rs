use noctis::prelude::*;

#[derive(Button)]
#[button("a", permissions = MANAGE_SERVER, defaults(style = Secondary, label = "Go"))]
struct Staff;

impl Staff {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
