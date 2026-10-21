use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("config", description = "d", permissions = MANAGE_SERVER)]
struct Config;

impl Config {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
