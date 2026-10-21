use noctis::prelude::*;

#[derive(Button)]
#[button("profile.edit", defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.defer_update().await?.done())
    }
}

fn main() {}
