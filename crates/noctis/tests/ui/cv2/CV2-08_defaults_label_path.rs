use noctis::prelude::*;

mod keys {
    pub const SAVE: &str = "shop.save";
}

#[derive(Button)]
#[button("a", defaults(style = Primary, label = keys::SAVE))]
struct Save;

fn main() {}
