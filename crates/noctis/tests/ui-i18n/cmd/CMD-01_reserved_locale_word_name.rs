// CMD-01: with locale files, a Command word cannot be `name` (nor
// `description` or `opt`): a one-word Path `name` is rejected.
use noctis::prelude::*;

#[derive(SlashCommand)]
#[command(
    "name",
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
