use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", description = "d", guilds = [42], contexts = [Guild])]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
