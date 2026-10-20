use noctis::prelude::*;

#[command("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
