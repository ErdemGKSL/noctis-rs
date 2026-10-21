use noctis::prelude::*;
use noctis::ui::Components;

#[derive(Button)]
#[button("profile.edit", defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new())
    }

    #[on_expired]
    async fn gone(
        _ctx: ComponentCtx,
        _why: Expired,
        r: ComponentResponder<Manual>,
    ) -> HandlerResult<Responded> {
        Ok(r.defer_update().await?.done())
    }
}

fn main() {}
