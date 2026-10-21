use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum One {
    #[choice("Only")]
    Only,
}

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[radio_group("Pick")]
    pick: One,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
