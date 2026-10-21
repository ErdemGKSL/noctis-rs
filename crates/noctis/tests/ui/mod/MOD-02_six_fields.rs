use noctis::prelude::*;

#[derive(Modal)]
#[modal("t.form", defaults(title = "Six"))]
struct F {
    #[checkbox("A")]
    a: bool,
    #[checkbox("B")]
    b: bool,
    #[checkbox("C")]
    c: bool,
    #[checkbox("D")]
    d: bool,
    #[checkbox("E")]
    e: bool,
    #[checkbox("F")]
    f: bool,
}

impl F {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
