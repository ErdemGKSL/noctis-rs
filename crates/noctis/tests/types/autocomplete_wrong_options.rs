use noctis::prelude::*;

#[derive(Options)]
struct Other {
    #[option(description = "d")]
    query: String,
}

#[autocomplete]
async fn suggest(ctx: AutocompleteCtx, _partial: Partial<Other>) -> HandlerResult<Responded> {
    Ok(ctx.respond(Vec::new()))
}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = suggest)]
    query: String,
}

fn main() {}
