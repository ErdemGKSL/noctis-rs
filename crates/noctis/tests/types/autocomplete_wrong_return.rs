use noctis::prelude::*;

#[autocomplete]
async fn suggest(_ctx: AutocompleteCtx) -> HandlerResult<()> {
    Ok(())
}

fn main() {}
