use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d", min_value = -9007199254740992)]
    a: i64,
}

impl O {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
