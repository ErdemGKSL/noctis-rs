use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", min_value = -9007199254740992)]
    a: i64,
}

fn main() {}
