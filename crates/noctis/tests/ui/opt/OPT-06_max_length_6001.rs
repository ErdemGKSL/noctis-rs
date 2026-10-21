use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d", max_length = 6001)]
    a: String,
}

impl O {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
