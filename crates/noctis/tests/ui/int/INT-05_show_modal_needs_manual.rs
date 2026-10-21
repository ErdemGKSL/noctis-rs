use noctis::prelude::*;
use noctis::ui::Modal;

fn form() -> Modal {
    unimplemented!()
}

#[derive(SlashCommand)]
#[command("feedback", description = "d")]
struct Feedback;

impl Feedback {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, r: CommandResponder) -> HandlerResult<Responded> {
        Ok(r.show_modal(form()).await?.done())
    }
}

fn main() {}
