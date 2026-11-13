use noctis::prelude::*;

#[derive(AppEmojis, Clone, Copy)]
#[emojis(dir = "../../fixtures/emoji", __dir_from_source_file)]
enum Icons {
    #[emoji(name = "st-online")]
    Online,
    X,
}

fn main() {}
