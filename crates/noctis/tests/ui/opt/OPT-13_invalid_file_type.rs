use noctis::prelude::*;

use noctis::ui::ResolvedAttachment;

#[derive(Options)]
struct O {
    #[option(description = "d", file_types = ["pdf"])]
    a: ResolvedAttachment,
}

fn main() {}
