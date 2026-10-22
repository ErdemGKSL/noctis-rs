use noctis::prelude::*;

mod keys {
    pub const L: &str = "form.bio";
}

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[text_input(keys::L)]
    bio: String,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
