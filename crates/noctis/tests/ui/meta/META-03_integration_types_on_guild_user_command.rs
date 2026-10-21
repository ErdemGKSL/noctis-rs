use noctis::prelude::*;

#[derive(UserCommand)]
#[user_command("Show profile", integration_types = [UserInstall], guilds = [7, 8])]
struct Show;

impl Show {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
