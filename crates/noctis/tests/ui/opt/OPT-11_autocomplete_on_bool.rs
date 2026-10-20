use noctis::prelude::*;

fn suggest() {}

#[derive(Options)]
struct O {
    #[option(description = "d", autocomplete = suggest)]
    a: bool,
}

fn main() {}
