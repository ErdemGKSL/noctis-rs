// CMD-04: a description literal must equal the default-locale file's text.
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command(
    "ping",
    description = "Ping",
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
