use noctis::prelude::*;

#[command("ping", description = "d", nsfw = 1)]
async fn ping(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
