use noctis::prelude::*;

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[text_input("Reason", min_length = 10, max_length = 5)]
    reason: String,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
