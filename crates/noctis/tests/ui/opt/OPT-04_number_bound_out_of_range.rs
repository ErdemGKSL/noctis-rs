use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", max_value = 1e300)]
    a: f64,
}

fn main() {}
