use noctis::prelude::*;

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy;

impl Buy {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: CommandResponder) -> HandlerResult<Responded> {
        Ok(r.defer_reply().await?.done())
    }
}

#[derive(Modal)]
#[modal("profile.bio", defaults(title = "Bio"))]
struct BioForm {
    #[text_input("Bio")]
    bio: String,
}

impl BioForm {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        Ok(r.defer_update().await?.done())
    }
}

fn main() {}
