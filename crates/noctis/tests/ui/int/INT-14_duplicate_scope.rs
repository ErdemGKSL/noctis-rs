use noctis::prelude::*;

#[derive(Modal)]
#[modal(
    "form",
    rate_limit(user, 3, per = "1s"),
    rate_limit(user, 5, per = "1m"),
    defaults(title = "Form")
)]
struct Form {
    #[text_input("Bio")]
    bio: Option<String>,
}

impl Form {
    #[handler]
    async fn handle(self, _ctx: ModalSubmitCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
