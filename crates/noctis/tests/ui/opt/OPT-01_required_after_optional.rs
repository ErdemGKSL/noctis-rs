use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d")]
    a: Option<i64>,
    #[option(description = "d")]
    b: String,
}

fn main() {}
