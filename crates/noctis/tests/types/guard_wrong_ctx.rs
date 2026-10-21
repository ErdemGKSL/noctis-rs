use noctis::prelude::*;

mod auth {
    use noctis::prelude::*;

    pub async fn modal_ctx(_ctx: &ModalSubmitCtx, _this: &super::Wrong) -> HandlerResult<Access> {
        Ok(Access::Allow)
    }

    pub async fn old_shape(_ctx: &ComponentCtx, _this: &super::Old) -> Result<(), MessageKey> {
        Ok(())
    }
}

#[derive(Button)]
#[button("a", guard = auth::modal_ctx, defaults(style = Secondary, label = "Go"))]
struct Wrong;

impl Wrong {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

#[derive(Button)]
#[button("b", guard = auth::old_shape, defaults(style = Secondary, label = "Go"))]
struct Old;

impl Old {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
