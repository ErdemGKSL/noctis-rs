use noctis::prelude::*;
use noctis::twilight::model::id::Id;
use noctis::twilight::model::id::marker::UserMarker;

#[derive(Modal)]
#[modal("bio", owner = bio, defaults(title = "Bio"))]
struct BioForm {
    user: Id<UserMarker>,
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
