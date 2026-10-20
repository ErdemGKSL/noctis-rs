use noctis::prelude::*;

#[command("config\tset", description = "d")]
async fn h(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
