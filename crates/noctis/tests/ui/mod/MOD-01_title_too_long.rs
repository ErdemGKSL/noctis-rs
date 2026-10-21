use noctis::prelude::*;

#[derive(Modal)]
#[modal("t.form", defaults(title = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"))]
struct F {
    #[checkbox("Agree")]
    agree: bool,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
