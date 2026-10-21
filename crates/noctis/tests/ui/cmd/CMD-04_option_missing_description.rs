use noctis::prelude::*;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    /// Doc comments are rustdoc only.
    days: i64,
}

fn main() {}
