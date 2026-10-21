use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")]
    days: i64,
}

fn main() {}
