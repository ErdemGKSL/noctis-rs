use noctis::prelude::*;
use noctis::ui::Components;

#[derive(Modal)]
#[modal("profile.bio", auto_defer = false, defaults(title = "Bio"))]
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, _r: ModalResponder) -> HandlerResult<Components> {
        Ok(Components::new())
    }
}

fn main() {}
