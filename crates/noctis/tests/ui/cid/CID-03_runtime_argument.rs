use noctis::prelude::*;

#[derive(Button)]
#[button("a", rate_limit(user, 5, per = "1m"), defaults(style = Secondary, label = "Go"))]
struct Limited;

impl Limited {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
