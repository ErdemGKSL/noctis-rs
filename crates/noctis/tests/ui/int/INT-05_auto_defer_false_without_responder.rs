use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("feedback", description = "d", auto_defer = false)]
struct Feedback;

impl Feedback {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::Components> {
        Ok(noctis::ui::Components::new())
    }
}

fn main() {}
