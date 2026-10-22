// TW-02: `theme!` on a missing file is a compile error at the call site.
use noctis::image::{Theme, theme};

fn main() {
    let _: Theme = theme!();
    let _: Theme = theme!("nope.css");
}
