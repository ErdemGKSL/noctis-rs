use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d")]
    a: i64,
    #[option(description = "d", rename = "a")]
    b: i64,
}

fn main() {}
