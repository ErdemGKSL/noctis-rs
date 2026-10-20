use noctis::prelude::*;

#[autocomplete]
async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
    ctx.defer();
    Ok(ctx.respond(Vec::new()))
}

fn main() {}
