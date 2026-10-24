// TW-02: a theme animation must name keyframes the file defines (or a
// built-in animation).
use noctis::image::{Theme, theme};

fn main() {
    let _: Theme = theme!("../../fixtures/theme/unknown_keyframes.css");
}
