use noctis::prelude::*;
use noctis::ui::Components;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[derive(Button)]
#[button("profile.edit", auto_defer = false, defaults(style = Secondary, label = "Edit"))]
struct EditBio;

impl EditBio {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(form()).await?.done())
    }

    // The hook inherits `auto_defer = false`: it must take the `Manual` responder.
    #[on_stale]
    async fn stale(_ctx: ComponentCtx) -> HandlerResult<Components> {
        Ok(Components::new())
    }
}

fn main() {}
