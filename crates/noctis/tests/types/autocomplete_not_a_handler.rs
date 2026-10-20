use noctis::prelude::*;

fn plain_fn() {}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = plain_fn)]
    query: String,
}

fn main() {}
