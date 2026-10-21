use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum TwentySix {
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
    #[choice("V12")]
    V12,
    #[choice("V13")]
    V13,
    #[choice("V14")]
    V14,
    #[choice("V15")]
    V15,
    #[choice("V16")]
    V16,
    #[choice("V17")]
    V17,
    #[choice("V18")]
    V18,
    #[choice("V19")]
    V19,
    #[choice("V20")]
    V20,
    #[choice("V21")]
    V21,
    #[choice("V22")]
    V22,
    #[choice("V23")]
    V23,
    #[choice("V24")]
    V24,
    #[choice("V25")]
    V25,
    #[choice("V26")]
    V26,
}

#[derive(Modal)]
#[modal("t.form", defaults(title = "T"))]
struct F {
    #[string_select("Pick")]
    pick: TwentySix,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
