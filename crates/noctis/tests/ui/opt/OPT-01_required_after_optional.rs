use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d")]
    a: Option<i64>,
    #[option(description = "d")]
    b: String,
}

fn main() {}
