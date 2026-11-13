use noctis::prelude::*;

#[derive(AppEmojis, Clone, Copy)]
#[emojis(dir = "../fixtures/emoji", __dir_from_source_file)]
enum Icons {
    #[emoji(render)]
    Logo,
}

fn main() {}
