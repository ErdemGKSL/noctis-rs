use noctis::prelude::*;

#[derive(Button)]
#[button("profile.edit", auto_defer = false, defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        Ok(r.defer_update().await?.done())
    }
}

fn main() {}
