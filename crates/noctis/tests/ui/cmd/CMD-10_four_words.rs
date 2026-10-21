use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("a b c d", description = "d")]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
