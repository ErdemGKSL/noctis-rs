use noctis::prelude::*;

#[command("ping", description = "d", guilds = [42], contexts = [Guild])]
async fn ping(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
