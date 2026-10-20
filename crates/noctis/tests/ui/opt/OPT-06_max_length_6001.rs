use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", max_length = 6001)]
    a: String,
}

fn main() {}
