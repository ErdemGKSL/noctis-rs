use noctis::prelude::*;

#[derive(Modal)]
#[modal("profile.bio", defaults(title = "Bio"))]
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, r: ModalResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

fn main() {}
