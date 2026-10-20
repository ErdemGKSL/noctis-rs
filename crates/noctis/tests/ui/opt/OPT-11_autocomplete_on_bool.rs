use noctis::prelude::*;

#[autocomplete]
async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
    Ok(ctx.respond(Vec::new()))
}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = suggest)]
    a: bool,
}

fn main() {}
