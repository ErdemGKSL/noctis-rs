use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", min_value = 1)]
    a: String,
}

fn main() {}
