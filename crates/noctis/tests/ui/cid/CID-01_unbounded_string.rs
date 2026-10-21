use noctis::prelude::*;

#[derive(Button)]
#[button("report.reason", defaults(style = Secondary, label = "Go"))]
struct Report {
    r: String,
}

impl Report {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
