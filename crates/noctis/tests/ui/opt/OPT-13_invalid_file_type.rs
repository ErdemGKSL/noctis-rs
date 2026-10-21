use noctis::prelude::*;

use noctis::ui::ResolvedAttachment;

#[derive(SlashCommand)]
#[command("o", description = "d")]
struct O {
    #[option(description = "d", file_types = ["pdf"])]
    a: ResolvedAttachment,
}

fn main() {}
