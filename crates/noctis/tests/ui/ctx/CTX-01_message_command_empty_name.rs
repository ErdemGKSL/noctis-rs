use noctis::prelude::*;

#[derive(MessageCommand)]
#[message_command("")]
struct Quote;

impl Quote {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _target: noctis::ui::ResolvedMessage) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
