use noctis::prelude::*;

#[derive(Button)]
#[button("a", response = edit, defaults(style = Secondary, label = "Go"))]
struct Edit;

impl Edit {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("b", auto_defer = 1, defaults(style = Secondary, label = "Go"))]
struct One;

impl One {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("c", response = public, response = update, defaults(style = Secondary, label = "Go"))]
struct Twice;

impl Twice {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
