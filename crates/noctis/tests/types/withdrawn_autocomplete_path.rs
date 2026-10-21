use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("find", description = "d")]
struct Find {
    #[option(description = "d", autocomplete = suggest)] // withdrawn form
    query: String,
}

fn main() {}
