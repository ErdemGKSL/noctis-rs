use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("b (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaab|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaac|aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaad) (bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb00|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb01|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb02|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb03|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb04|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb05|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb06|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb07|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb08|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb09|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb10|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb11|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb12|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb13|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb14|bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb15)",
    description = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd",
    max_expansions = 64
)]
struct H;

impl H {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _path: HPath) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
