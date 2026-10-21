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

    #[autocomplete(query)]
    async fn query(ctx: AutocompleteCtx, _typed: &str, _filled: Partial<Self>) -> HandlerResult<Suggestions<String>> {
        ctx.defer_reply(); // autocomplete has no defer (INT-06)
        Ok(Suggestions::new())
    }
}

fn main() {}
