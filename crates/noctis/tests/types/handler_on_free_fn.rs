use noctis::prelude::*;

// Story 6.11 Matrix 8 and 53: `#[handler]` marks a method taking `self`.
#[handler] // withdrawn form
async fn handle(_ctx: ComponentCtx) -> HandlerResult<Responded> {
    unimplemented!()
}

#[derive(Button)]
#[button("a", defaults(style = Primary, label = "A"))]
struct ByRef;

impl ByRef {
    #[handler]
    async fn handle(&self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("b", defaults(style = Primary, label = "B"))]
struct NotAsync;

impl NotAsync {
    #[handler]
    fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("c", defaults(style = Primary, label = "C"))]
struct WithArgs;

impl WithArgs {
    #[handler(response = update)]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
