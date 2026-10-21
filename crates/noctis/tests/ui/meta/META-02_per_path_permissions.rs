use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("config (set|get)", description = "d", permissions(set = MANAGE_GUILD))]
struct Config;

impl Config {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: ConfigPath) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
