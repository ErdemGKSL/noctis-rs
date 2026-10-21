use noctis::prelude::*;

#[derive(Modal)]
#[modal("t.form", defaults(title = "Empty"))]
struct F {}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
