// CMD-04: with a default-locale file, a path without a literal needs
// `cmd.<path>.description` in it.
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("pong", locales = "../../fixtures/i18n/cmdkeys", __locales_from_source_file)]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
