use noctis::prelude::*;

#[derive(Button)]
#[button("t.press", defaults(style = Secondary, label = "Go"))]
struct Press {
    n: u8,
}

impl Press {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = components! { row [ Press { n: 1 }.build(), Press { n: 1 }.build().label("x") ] };
}
