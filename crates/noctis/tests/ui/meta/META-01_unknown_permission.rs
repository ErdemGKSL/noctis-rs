use noctis::prelude::*;

#[command("config", description = "d", permissions = MANAGE_SERVER)]
async fn config(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
