use noctis::prelude::*;

#[derive(Options)]
struct O {
    #[option(description = "d", channel_types = [GuildText])]
    a: String,
}

fn main() {}
