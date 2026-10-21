use noctis::prelude::*;

#[derive(Button)]
#[button("abcdefghijklmnopqrstuvwxy", defaults(style = Secondary, label = "Go"))]
struct Long;

impl Long {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
