use noctis::prelude::*;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[derive(Modal)]
#[modal("profile.bio", auto_defer = false, defaults(title = "Bio"))]
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, r: ModalResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.show_modal(form()).await?.done()) // [INT-04]
    }
}

fn main() {}
