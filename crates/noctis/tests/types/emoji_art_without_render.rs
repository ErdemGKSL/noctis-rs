use noctis::prelude::*;

fn art(_: Icons, _: EmojiTone) -> EmojiArt {
    unimplemented!()
}

#[derive(AppEmojis, Clone, Copy)]
#[emojis(dir = "../fixtures/emoji", art = art, __dir_from_source_file)]
enum Icons {
    Check,
}

fn main() {}
