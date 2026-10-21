use noctis::prelude::*;

#[derive(UserCommand)]
#[user_command("Show profile")]
struct ShowProfile;

impl ShowProfile {
    #[handler]
    async fn handle(self, _ctx: CommandCtx, _target: noctis::ui::ResolvedUser) -> HandlerResult<u8> {
        Ok(1)
    }
}

fn main() {}
