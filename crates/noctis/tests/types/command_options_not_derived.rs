use noctis::prelude::*;

#[command("warn", description = "d")]
async fn warn(_ctx: CommandCtx, _options: String) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
