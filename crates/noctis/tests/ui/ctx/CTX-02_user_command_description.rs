use noctis::prelude::*;

#[derive(UserCommand)]
#[user_command("Show profile", description = "Shows a profile")] // [CTX-02]
struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<noctis::ui::LegacyMessage> {
        Ok(noctis::ui::LegacyMessage::new("ok"))
    }
}

fn main() {}
