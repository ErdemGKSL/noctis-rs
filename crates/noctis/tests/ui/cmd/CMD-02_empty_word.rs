use noctis::prelude::*;

#[command("config  set", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
