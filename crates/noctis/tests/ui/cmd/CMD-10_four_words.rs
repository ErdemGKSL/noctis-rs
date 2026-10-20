use noctis::prelude::*;

#[command("a b c d", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
