use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("warn", description = "d")]
struct Warn {
    #[option(description = "d", autocomplete)]
    rule: i64,
}

impl Warn {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }
    // [OPT-11]: no `#[autocomplete(rule)]` method.
}

fn main() {}
