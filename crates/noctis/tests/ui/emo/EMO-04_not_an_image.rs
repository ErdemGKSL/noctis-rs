use noctis::prelude::*;

#[derive(AppEmojis, Clone, Copy)]
#[emojis(dir = "../../fixtures/emoji", __dir_from_source_file)]
enum Icons {
    Fake,
    #[emoji(file = "check.png")]
    Ok,
}

fn main() {}
