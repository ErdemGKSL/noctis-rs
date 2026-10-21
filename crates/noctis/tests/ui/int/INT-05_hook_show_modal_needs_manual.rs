use noctis::prelude::*;
use noctis::ui::Components;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[derive(Button)]
#[button("profile.edit", defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new())
    }

    #[on_stale]
    async fn stale(_ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        Ok(r.show_modal(form()).await?.done())
    }
}

fn main() {}
