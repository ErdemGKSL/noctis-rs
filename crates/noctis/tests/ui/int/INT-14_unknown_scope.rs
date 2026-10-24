use noctis::prelude::*;

#[derive(Button)]
#[button("a", rate_limit(users, 3, per = "1s"), defaults(style = Secondary, label = "Go"))]
struct Limited;

impl Limited {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
