use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", min_value = 10, max_value = 5)]
    a: i64,
}

fn main() {}
