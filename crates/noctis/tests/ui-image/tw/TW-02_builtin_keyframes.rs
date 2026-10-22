// TW-02: an invalid theme file is a compile error at the call site.
use noctis::image::{Theme, theme};

fn main() {
    let _: Theme = theme!("../../fixtures/theme/builtin_keyframes.css");
}
