use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("ping", description = "d", response = update)] // [INT-04]
struct Ping;

impl Ping {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<noctis::ui::Components> {
        Ok(noctis::ui::Components::new())
    }
}

fn main() {}
