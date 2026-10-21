use noctis::prelude::*;

/// Doc comments never become the description.
#[derive(SlashCommand)]
#[command("ping")]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
