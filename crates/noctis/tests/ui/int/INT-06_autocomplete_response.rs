use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("find", description = "d")]
struct Find {
    #[option(description = "d", autocomplete)]
    query: String,
}

impl Find {
    #[handler]
    async fn handle(self, _ctx: CommandCtx) -> HandlerResult<Responded> {
        unimplemented!()
    }

    #[autocomplete(query, response = ephemeral)] // [INT-06]
    async fn query(_ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Suggestions<String>> {
        Ok(Suggestions::new())
    }
}

fn main() {}
