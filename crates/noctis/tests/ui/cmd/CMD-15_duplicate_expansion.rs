use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("a (b|b)", description = "d")]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: HPath) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
