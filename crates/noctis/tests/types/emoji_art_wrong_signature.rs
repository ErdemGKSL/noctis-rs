use noctis::prelude::*;

fn art(_: Icons) -> EmojiArt {
    unimplemented!()
}

#[derive(AppEmojis, Clone, Copy)]
#[emojis(art = art)]
enum Icons {
    #[emoji(render)]
    Logo,
}

fn main() {}
