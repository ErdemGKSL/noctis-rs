use noctis::prelude::*;

#[derive(Modal)]
#[modal("profile.bio", auto_defer = false, defaults(title = "Bio"))]
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, r: ModalResponder<Manual>) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }

    // The hook inherits `auto_defer = false`: it must take the Manual responder.
    #[on_stale]
    async fn stale(_ctx: ModalSubmitCtx, r: ModalResponder) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

fn main() {}
