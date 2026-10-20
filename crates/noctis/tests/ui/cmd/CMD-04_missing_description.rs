use noctis::prelude::*;

/// Doc comments never become the description.
#[command("ping")]
async fn h(_ctx: CommandCtx) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
