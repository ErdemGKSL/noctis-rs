use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("warn", description = "d")]
struct Warn {
    #[option(description = "d")]
    rule: i64,
}

impl Warn {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }

    #[autocomplete(nope)] // [OPT-11]: no such field
    async fn nope(_ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Suggestions<i64>> {
        Ok(Suggestions::new())
    }
}

fn main() {}
