use noctis::prelude::*;
use noctis::ui::LegacyMessage;

#[derive(Button)]
#[button("shop.buy", defaults(style = Primary, label = "Buy"))]
struct Buy {
    item: u64,
}

impl Buy {
    #[handler]
    async fn handle(self, _ctx: ComponentCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("x")) // [CV2-01]
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
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<LegacyMessage> {
        Ok(LegacyMessage::new("x")) // [CV2-01]
    }
}

fn main() {}
