use noctis::prelude::*;

#[derive(Choices, Clone, Copy)]
enum Severity {
    #[choice("Low")]
    Low,
}

#[autocomplete]
async fn suggest(ctx: AutocompleteCtx) -> HandlerResult<Responded> {
    Ok(ctx.respond(Vec::new()))
}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = suggest)]
    a: Severity,
}

fn main() {}
