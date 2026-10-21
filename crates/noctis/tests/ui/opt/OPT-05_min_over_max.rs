use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d", min_value = 10, max_value = 5)]
    a: i64,
}

impl O {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
}

fn main() {}
