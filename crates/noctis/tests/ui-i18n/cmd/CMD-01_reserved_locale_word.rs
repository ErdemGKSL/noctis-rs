// CMD-01: with locale files, a Command word cannot be `name`, `description`
// or `opt`.
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command(
    "config opt",
    description = "d",
    locales = "../../fixtures/i18n/cmdkeys",
    __locales_from_source_file
)]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
