use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", description = "d", dm_permission = false)] // [META-05]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
