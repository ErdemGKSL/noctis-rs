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

#[derive(StringSelectMenu)]
#[string_select("t.pick", defaults(placeholder = "Pick"))]
struct Pick;

impl Pick {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {
    let _ = components! { row [ Pick.build(), Press { n: 1 }.build() ] };
}
