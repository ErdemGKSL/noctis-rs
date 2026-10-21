use noctis::prelude::*;

#[derive(Button)]
#[button("a", guard = "auth::is_staff", defaults(style = Secondary, label = "Go"))]
struct Staff;

impl Staff {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
