use noctis::prelude::*;

mod keys {
    pub const T: &str = "form.title";
}

#[derive(Modal)]
#[modal("t.form", defaults(title = keys::T))]
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
