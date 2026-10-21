use noctis::prelude::*;
use noctis::ui::LegacyMessage;

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx, r: ComponentResponder) -> HandlerResult<Responded> {
        let acked = r.update(LegacyMessage::new("x")).await?; // [CV2-01]
        acked.edit_reply(LegacyMessage::new("y")).await?; // [CV2-01]
        Ok(acked.done())
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
    async fn handle(self, _ctx: ModalSubmitCtx, r: ModalResponder) -> HandlerResult<Responded> {
        Ok(r.update(LegacyMessage::new("x")).await?.done()) // [CV2-01]
    }
}

fn main() {}
