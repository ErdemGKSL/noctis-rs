use noctis::prelude::*;

#[message_command("")]
async fn quote(_ctx: CommandCtx, _target: noctis::ui::ResolvedMessage) -> HandlerResult<noctis::ui::LegacyMessage> {
    Ok(noctis::ui::LegacyMessage::new("ok"))
}

fn main() {}
