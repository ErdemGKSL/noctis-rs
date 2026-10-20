use noctis::prelude::*;

#[autocomplete(auto_defer = false)] // [INT-06]
async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
    Ok(ctx.respond(Vec::new()))
}

fn main() {}
