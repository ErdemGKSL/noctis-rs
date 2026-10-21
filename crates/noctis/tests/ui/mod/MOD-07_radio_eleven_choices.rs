use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum Eleven {
    #[choice("V1")]
    V1,
    #[choice("V2")]
    V2,
    #[choice("V3")]
    V3,
    #[choice("V4")]
    V4,
    #[choice("V5")]
    V5,
    #[choice("V6")]
    V6,
    #[choice("V7")]
    V7,
    #[choice("V8")]
    V8,
    #[choice("V9")]
    V9,
    #[choice("V10")]
    V10,
    #[choice("V11")]
    V11,
}

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[radio_group("Pick")]
    pick: Eleven,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
