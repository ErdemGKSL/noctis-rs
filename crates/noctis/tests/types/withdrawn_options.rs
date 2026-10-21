use noctis::prelude::*;

#[derive(Options)] // withdrawn form
struct WarnOptions {
    #[option(description = "d")]
    reason: String,
}

fn main() {}
